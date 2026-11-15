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
    /// Serves a folder from the Mac, mounted here (an app server); the
    /// folder is asked for, and the service runs as the Mac user.
    folder: Option<&'static str>,
}

const RECIPES: [Recipe; 11] = [
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
        folder: None,
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
        folder: None,
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
        folder: None,
    },
    Recipe {
        id: "elyrasql",
        title: "ElyraSQL",
        about: "ElyraSQL 1.12: MySQL-compatible, one file, with vector search. Its one database is elyra. User root, password elyrasql.",
        image: "ghcr.io/kwhorne/elyrasql:1.12",
        port: 3307,
        ui_port: None,
        env: &["ELYRASQL_USER=root", "ELYRASQL_PASSWORD=elyrasql"],
        cmd: &[],
        data: Some("/var/lib/elyrasql"),
        connect: "mysql://root:elyrasql@localhost:{port}/elyra",
        ui: None,
        folder: None,
    },
    Recipe {
        id: "sqlanywhere",
        title: "SQL Anywhere",
        about: "SQL Anywhere 0.11 (sqld): SQLite over HTTP and WebSockets, with vector search and replicas. No password; this Mac only.",
        image: "ghcr.io/kwhorne/sqlanywhere-server:0.11",
        port: 8080,
        ui_port: None,
        env: &[],
        cmd: &[],
        data: Some("/var/lib/sqld"),
        connect: "http://localhost:{port}",
        ui: None,
        folder: None,
    },
    Recipe {
        id: "askr",
        title: "Askr",
        about: "Askr 1.9: a PHP 8.5 app server (no FPM, no nginx) for a folder on your Mac. Laravel runs in worker mode: booted once, served many.",
        image: "ghcr.io/kwhorne/askr:1.9",
        port: 8080,
        ui_port: None,
        env: &["ASKR_APP_BASE=/var/www/app"],
        cmd: &[],
        data: None,
        connect: "http://localhost:{port}",
        ui: None,
        folder: Some("/var/www/app"),
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
        folder: None,
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
        folder: None,
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
        folder: None,
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
        folder: None,
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
        folder: None,
    },
];

#[derive(Serialize, Deserialize, specta::Type)]
pub struct RecipeRow {
    pub id: String,
    pub title: String,
    pub about: String,
    pub image: String,
    /// It serves a folder from the Mac, which `recipe_start` needs.
    pub needs_folder: bool,
}

/// The services a recipe starts: PostgreSQL, MySQL, MariaDB, ElyraSQL
/// (MySQL-compatible, with vector search), SQL Anywhere (SQLite over HTTP),
/// Askr (a PHP app server for a folder on the Mac), Redis, MongoDB, RabbitMQ,
/// Mailpit (a mail catcher) and MinIO (S3).
#[command(can = "edock.view")]
pub async fn recipes_list(_ctx: Ctx) -> Vec<RecipeRow> {
    RECIPES.iter().map(|r| RecipeRow { id: r.id.into(), title: r.title.into(), about: r.about.into(), image: r.image.into(), needs_folder: r.folder.is_some() }).collect()
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

/// Folders the engines share from the Mac (eDock's, Docker Desktop's).
const SHARED: [&str; 4] = ["/Users/", "/Volumes/", "/private/", "/tmp/"];

/// A folder from the Mac to serve: there, a folder, and shared with the
/// engine. Its real path.
fn mac_folder(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("Choose the project's folder".into());
    }
    let real = std::fs::canonicalize(p).map_err(|_| format!("{path} isn't there"))?;
    if !real.is_dir() {
        return Err(format!("{path} isn't a folder"));
    }
    let shown = real.to_string_lossy();
    if !SHARED.iter().any(|s| shown.starts_with(s)) {
        return Err(format!("{path} isn't under /Users or /Volumes, which containers can reach"));
    }
    Ok(real)
}

/// A container name from a folder's: `My App` → `my-app`.
fn name_from(dir: &std::path::Path) -> String {
    let raw = dir.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let s: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "app".into() } else { s }
}

/// How Askr serves a project: a Laravel app (it has `artisan`) in worker
/// mode, booted once; any other PHP from its `public/`, or the folder itself.
fn askr_cmd(dir: &std::path::Path, mount: &str) -> Vec<String> {
    let root = if dir.join("public").is_dir() { format!("{mount}/public") } else { mount.to_owned() };
    let mut cmd = vec!["serve".to_string(), "--listen".into(), "0.0.0.0:8080".into(), "--root".into(), root, "--admin".into(), "127.0.0.1:9000".into(), "--access-log".into(), "-".into()];
    if dir.join("artisan").is_file() {
        cmd.extend(["--worker-script".into(), "/opt/askr/examples/laravel-worker.php".into(), "--workers".into(), "2".into()]);
    }
    cmd
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
/// with the engine. Returns how to connect. `name` defaults to the recipe's
/// id (to the folder's name for one that serves a folder). `folder`: the Mac
/// folder an app server serves (Askr), required for those.
#[command(can = "edock.manage")]
pub async fn recipe_start(ctx: Ctx, recipe: String, name: Option<String>, folder: Option<String>) -> Result<Started, String> {
    let r = RECIPES.iter().find(|r| r.id == recipe).ok_or_else(|| format!("There's no recipe {recipe}"))?;
    let project = match (r.folder, folder.as_deref().filter(|f| !f.trim().is_empty())) {
        (Some(_), Some(f)) => Some(mac_folder(f)?),
        (Some(_), None) => return Err(format!("{} serves a folder from your Mac: choose one", r.title)),
        (None, _) => None,
    };
    let docker = ctx.get::<Engine>().docker()?;
    let containers: Vec<String> = docker
        .list_containers(Some(q::ListContainersOptionsBuilder::default().all(true).build()))
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|c| c.names.as_ref()?.first().map(|n| n.trim_start_matches('/').to_owned()))
        .collect();
    let volumes: Vec<String> = docker.list_volumes(None::<q::ListVolumesOptions>).await.map_err(|e| e.to_string())?.volumes.unwrap_or_default().into_iter().map(|v| v.name).collect();
    let from_folder = project.as_deref().map(name_from);
    let want = name.as_deref().map(str::trim).filter(|n| !n.is_empty()).or(from_folder.as_deref()).unwrap_or(r.id);
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
    // Its domain (<name>.edock.localhost) opens the web UI, or the app.
    if let Some(p) = r.ui_port.or(r.folder.map(|_| r.port)) {
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
        cmd: match (&project, r.folder) {
            (Some(dir), Some(mount)) => Some(askr_cmd(dir, mount)),
            _ => (!r.cmd.is_empty()).then(|| r.cmd.iter().map(|s| (*s).to_owned()).collect()),
        },
        // A folder from the Mac is written as its owner (Laravel's storage/).
        user: project.as_ref().map(|_| {
            // SAFETY: getuid and getgid cannot fail.
            let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
            format!("{uid}:{gid}")
        }),
        labels: Some(labels),
        host_config: Some(bollard::models::HostConfig {
            port_bindings: Some(bindings),
            binds: match (&project, r.folder, r.data) {
                (Some(dir), Some(mount), _) => Some(vec![format!("{}:{mount}", dir.display())]),
                (_, _, Some(d)) => Some(vec![format!("{volume}:{d}")]),
                _ => None,
            },
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
    fn askr_serves_laravel_in_worker_mode_and_plain_php_from_public() {
        let dir = std::env::temp_dir().join(format!("edock-askr-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("public")).unwrap();
        let plain = super::askr_cmd(&dir, "/var/www/app");
        assert!(plain.windows(2).any(|w| w[0] == "--root" && w[1] == "/var/www/app/public"));
        assert!(!plain.iter().any(|a| a == "--worker-script"));
        std::fs::write(dir.join("artisan"), "").unwrap();
        let laravel = super::askr_cmd(&dir, "/var/www/app");
        assert!(laravel.windows(2).any(|w| w[0] == "--worker-script" && w[1].ends_with("laravel-worker.php")));
        assert_eq!(super::name_from(std::path::Path::new("/Users/x/My App_2")), "my-app-2");
        assert!(super::mac_folder("relative").is_err());
        assert!(super::mac_folder("/usr").is_err(), "not shared with the engine");
        assert!(super::mac_folder(dir.join("artisan").to_str().unwrap()).is_err(), "a file");
        let _ = std::fs::remove_dir_all(&dir);
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
