//! Recipes: a common development service in one step: a database, a
//! cache, a queue, a mail catcher. Each starts with settings that work for
//! development (a known password, its data in a named volume, its port on
//! the Mac), and says how to connect to it. Any engine.

use std::collections::HashMap;

use bollard::query_parameters as q;
use elyra::{command, Ctx, EventBus};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

const LABEL: &str = "app.edock.recipe";

/// One service: what it runs and how it's reached. `{port}` and `{ui}` in
/// `connect` and `ui` become the Mac ports it got.
struct Recipe {
    id: &'static str,
    title: &'static str,
    about: &'static str,
    image: &'static str,
    /// The main port (connect's), then a web UI's, if any.
    port: u16,
    ui_port: Option<u16>,
    env: &'static [&'static str],
    cmd: &'static [&'static str],
    /// Where its data lives, kept in the volume `<name>-data`.
    data: Option<&'static str>,
    connect: &'static str,
    ui: Option<&'static str>,
}

const RECIPES: [Recipe; 8] = [
    Recipe {
        id: "postgres",
        title: "PostgreSQL",
        about: "PostgreSQL 17. User and password postgres.",
        image: "postgres:17-alpine",
        port: 5432,
        ui_port: None,
        env: &["POSTGRES_PASSWORD=postgres"],
        cmd: &[],
        data: Some("/var/lib/postgresql/data"),
        connect: "postgres://postgres:postgres@localhost:{port}/postgres",
        ui: None,
    },
    Recipe {
        id: "mysql",
        title: "MySQL",
        about: "MySQL 8.4, with a database app. User root, password mysql.",
        image: "mysql:8.4",
        port: 3306,
        ui_port: None,
        env: &["MYSQL_ROOT_PASSWORD=mysql", "MYSQL_DATABASE=app"],
        cmd: &[],
        data: Some("/var/lib/mysql"),
        connect: "mysql://root:mysql@localhost:{port}/app",
        ui: None,
    },
    Recipe {
        id: "mariadb",
        title: "MariaDB",
        about: "MariaDB 11, with a database app. User root, password mariadb.",
        image: "mariadb:11",
        port: 3306,
        ui_port: None,
        env: &["MARIADB_ROOT_PASSWORD=mariadb", "MARIADB_DATABASE=app"],
        cmd: &[],
        data: Some("/var/lib/mysql"),
        connect: "mysql://root:mariadb@localhost:{port}/app",
        ui: None,
    },
    Recipe {
        id: "redis",
        title: "Redis",
        about: "Redis 8, saving to its volume.",
        image: "redis:8-alpine",
        port: 6379,
        ui_port: None,
        env: &[],
        cmd: &["redis-server", "--appendonly", "yes"],
        data: Some("/data"),
        connect: "redis://localhost:{port}",
        ui: None,
    },
    Recipe {
        id: "mongodb",
        title: "MongoDB",
        about: "MongoDB 8, without a password.",
        image: "mongo:8",
        port: 27017,
        ui_port: None,
        env: &[],
        cmd: &[],
        data: Some("/data/db"),
        connect: "mongodb://localhost:{port}",
        ui: None,
    },
    Recipe {
        id: "rabbitmq",
        title: "RabbitMQ",
        about: "RabbitMQ 4 with its management UI. User and password guest.",
        image: "rabbitmq:4-management-alpine",
        port: 5672,
        ui_port: Some(15672),
        env: &[],
        cmd: &[],
        data: Some("/var/lib/rabbitmq"),
        connect: "amqp://guest:guest@localhost:{port}",
        ui: Some("http://localhost:{ui}"),
    },
    Recipe {
        id: "mailpit",
        title: "Mailpit",
        about: "Catches the mail your app sends (SMTP), and shows it in a web UI.",
        image: "axllent/mailpit:latest",
        port: 1025,
        ui_port: Some(8025),
        env: &[],
        cmd: &[],
        data: None,
        connect: "smtp://localhost:{port}",
        ui: Some("http://localhost:{ui}"),
    },
    Recipe {
        id: "minio",
        title: "MinIO",
        about: "S3-compatible storage, with its console. User minio, password minio123.",
        image: "minio/minio:latest",
        port: 9000,
        ui_port: Some(9001),
        env: &["MINIO_ROOT_USER=minio", "MINIO_ROOT_PASSWORD=minio123"],
        cmd: &["server", "/data", "--console-address", ":9001"],
        data: Some("/data"),
        connect: "http://localhost:{port}",
        ui: Some("http://localhost:{ui}"),
    },
];

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RecipeRow {
    pub id: String,
    pub title: String,
    pub about: String,
    pub image: String,
}

/// The services a recipe starts: PostgreSQL, MySQL, MariaDB, Redis,
/// MongoDB, RabbitMQ, Mailpit (a mail catcher) and MinIO (S3).
#[command(can = "edock.view")]
pub async fn recipes_list(_ctx: Ctx) -> Vec<RecipeRow> {
    RECIPES.iter().map(|r| RecipeRow { id: r.id.into(), title: r.title.into(), about: r.about.into(), image: r.image.into() }).collect()
}

/// A port on the Mac nothing listens on, from `want` up.
fn free_port(want: u16, taken: &[u16]) -> u16 {
    (want..want.saturating_add(200))
        .find(|p| !taken.contains(p) && std::net::TcpListener::bind(("0.0.0.0", *p)).is_ok() && std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .unwrap_or(want)
}

/// `name`, or `name-2`, `name-3`… when a container or volume has it.
fn free_name(want: &str, containers: &[String], volumes: &[String]) -> String {
    let used = |n: &str| containers.iter().any(|c| c == n) || volumes.iter().any(|v| *v == format!("{n}-data"));
    if !used(want) {
        return want.to_owned();
    }
    (2..).map(|i| format!("{want}-{i}")).find(|n| !used(n)).expect("some name is free")
}

#[derive(Serialize, Deserialize, specta::Type)]
pub struct Started {
    pub container: String,
    /// How an app connects: `postgres://postgres:postgres@localhost:5432/postgres`.
    pub connect: String,
    /// Its web UI, if it has one.
    pub ui: Option<String>,
    pub port: u16,
}

/// Start a recipe's service (see `recipes_list`): pulls its image if
/// needed, keeps its data in the volume `<name>-data`, publishes its port on
/// the Mac's loopback (the usual one, or the next free one) and restarts it
/// with the engine. Returns how to connect. `name` defaults to the recipe's id.
#[command(can = "edock.manage")]
pub async fn recipe_start(ctx: Ctx, recipe: String, name: Option<String>) -> Result<Started, String> {
    let r = RECIPES.iter().find(|r| r.id == recipe).ok_or_else(|| format!("There's no recipe {recipe}"))?;
    let docker = ctx.get::<Engine>().docker()?;
    let containers: Vec<String> = docker
        .list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).build()))
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|c| c.names.as_ref()?.first().map(|n| n.trim_start_matches('/').to_owned()))
        .collect();
    let volumes: Vec<String> = docker.list_volumes(None::<q::ListVolumesOptions>).await.map_err(|e| e.to_string())?.volumes.unwrap_or_default().into_iter().map(|v| v.name).collect();
    let want = name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(r.id);
    if !want.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) || !want.as_bytes()[0].is_ascii_alphanumeric() {
        return Err("A name is letters, digits, dashes, dots and underscores".into());
    }
    let name = free_name(want, &containers, &volumes);

    if docker.inspect_image(r.image).await.is_err() {
        let _ = ctx.get::<EventBus>().emit("recipe:progress", &format!("Pulling {}…", r.image));
        let (repo, tag) = r.image.rsplit_once(':').unwrap_or((r.image, "latest"));
        let mut pull = docker.create_image(Some(q::CreateImageOptionsBuilder::default().from_image(repo).tag(tag).build()), None, None);
        while let Some(p) = pull.next().await {
            p.map_err(|e| format!("couldn't pull {}: {e}", r.image))?;
        }
    }

    let port = free_port(r.port, &[]);
    let ui_port = r.ui_port.map(|p| free_port(p, &[port]));
    let mut bindings = HashMap::new();
    // This Mac only: the passwords are well known, so not on the network.
    let bind = |p: u16| Some(vec![bollard::models::PortBinding { host_ip: Some("127.0.0.1".into()), host_port: Some(p.to_string()) }]);
    bindings.insert(format!("{}/tcp", r.port), bind(port));
    if let (Some(inner), Some(outer)) = (r.ui_port, ui_port) {
        bindings.insert(format!("{inner}/tcp"), bind(outer));
    }
    let mut labels: HashMap<String, String> = [(LABEL.to_string(), r.id.to_string())].into();
    // Its domain (<name>.edock.localhost) opens the web UI.
    if let Some(p) = r.ui_port {
        labels.insert("app.edock.http.port".into(), p.to_string());
    }
    let volume = format!("{name}-data");
    if r.data.is_some() {
        docker
            .create_volume(bollard::models::VolumeCreateRequest { name: Some(volume.clone()), labels: Some([(LABEL.to_string(), r.id.to_string())].into()), ..Default::default() })
            .await
            .map_err(|e| e.to_string())?;
    }
    let body = bollard::models::ContainerCreateBody {
        image: Some(r.image.into()),
        env: Some(r.env.iter().map(|s| (*s).to_owned()).collect()),
        cmd: (!r.cmd.is_empty()).then(|| r.cmd.iter().map(|s| (*s).to_owned()).collect()),
        labels: Some(labels),
        host_config: Some(bollard::models::HostConfig {
            port_bindings: Some(bindings),
            binds: r.data.map(|d| vec![format!("{volume}:{d}")]),
            restart_policy: Some(bollard::models::RestartPolicy { name: Some(bollard::models::RestartPolicyNameEnum::UNLESS_STOPPED), maximum_retry_count: None }),
            ..Default::default()
        }),
        ..Default::default()
    };
    docker.create_container(Some(q::CreateContainerOptionsBuilder::default().name(&name).build()), body).await.map_err(|e| e.to_string())?;
    if let Err(e) = docker.start_container(&name, None::<q::StartContainerOptions>).await {
        return Err(format!("{name} was made but didn't start: {e}"));
    }
    let fill = |s: &str| s.replace("{port}", &port.to_string()).replace("{ui}", &ui_port.unwrap_or(0).to_string());
    let _ = ctx.get::<EventBus>().emit("docker:changed", &crate::streams::Changed { kind: "container".into(), action: "start".into() });
    Ok(Started { container: name, connect: fill(r.connect), ui: r.ui.map(fill), port })
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_and_ports_dont_clash() {
        let c = vec!["postgres".to_string(), "postgres-2".to_string()];
        assert_eq!(super::free_name("postgres", &c, &[]), "postgres-3");
        assert_eq!(super::free_name("redis", &c, &["redis-data".to_string()]), "redis-2");
        assert_eq!(super::free_name("mongodb", &c, &[]), "mongodb");
        let held = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let p = held.local_addr().unwrap().port();
        assert_ne!(super::free_port(p, &[]), p);
        assert_ne!(super::free_port(p + 1, &[p + 1]), p + 1);
    }

    #[test]
    fn every_recipe_is_whole() {
        for r in &super::RECIPES {
            assert!(r.connect.contains("{port}"), "{}", r.id);
            assert_eq!(r.ui.is_some(), r.ui_port.is_some(), "{}", r.id);
            assert!(r.image.contains(':'), "{}", r.id);
        }
    }
}
