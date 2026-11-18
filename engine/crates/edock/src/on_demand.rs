//! `edock on-demand`: which Compose projects and containers stop while
//! nobody uses them and start again on the next connection, and for how
//! long they may be quiet first. What it sets goes in
//! `~/.edock/on-demand.json`, over the `app.edock.on-demand` label.

use std::io;

use serde_json::{Value, json};

use crate::client;

const USAGE: &str = "usage: edock on-demand [--all] | NAME | NAME TIME|off|reset   (TIME: 30s, 15m, 1h)";

pub fn run(args: &[String]) -> io::Result<()> {
    if !client::alive() {
        return Err(io::Error::other("edock daemon is not running: open eDock.app or run `edock daemon`"));
    }
    match args {
        [] => list(false),
        [all] if all == "--all" => list(true),
        [flag] if flag.starts_with('-') => Err(io::Error::other(USAGE)),
        [name] => {
            let units = units()?;
            let u = units.iter().find(|u| u["name"] == name.as_str()).ok_or_else(|| io::Error::other(format!("there's no Compose project or container named {name}")))?;
            print_table(std::slice::from_ref(u));
            Ok(())
        }
        [name, value] => {
            let value = if value == "reset" { Value::Null } else { json!(value) };
            let r = client::request("POST", "/edock/on-demand", Some(&json!({"name": name, "value": value})))?;
            if !r.ok() {
                return Err(io::Error::other(r.error()));
            }
            let u = r.json();
            println!("{}", sentence(&u));
            Ok(())
        }
        _ => Err(io::Error::other(USAGE)),
    }
}

fn units() -> io::Result<Vec<Value>> {
    Ok(client::get("/edock/on-demand")?.as_array().cloned().unwrap_or_default())
}

fn list(all: bool) -> io::Result<()> {
    let units: Vec<Value> = units()?.into_iter().filter(|u| all || u["on"] == true).collect();
    if units.is_empty() {
        println!(
            "{}",
            if all {
                "No Compose projects or containers."
            } else {
                "Nothing is on demand. Turn a project on with `edock on-demand NAME 15m`, or the label app.edock.on-demand=15m on one of its services."
            }
        );
        return Ok(());
    }
    print_table(&units);
    Ok(())
}

/// What On demand is for one, in a few words: `after 15m (label)`, `off`.
fn on_demand(u: &Value) -> String {
    let from = u["from"].as_str().map(|f| format!(" ({f})")).unwrap_or_default();
    match u["after"].as_str() {
        Some(after) => format!("after {after}{from}"),
        None if u["setting"] == "off" => format!("off{from}"),
        None if u["label"].is_string() && u["wakeable"] == false => "off (nothing can wake it: publish a port)".into(),
        None => "off".into(),
    }
}

fn print_table(units: &[Value]) {
    let rows: Vec<[String; 4]> = units
        .iter()
        .map(|u| [u["name"].as_str().unwrap_or("").to_owned(), u["kind"].as_str().unwrap_or("").to_owned(), on_demand(u), u["state"].as_str().unwrap_or("").to_owned()])
        .collect();
    let head = ["NAME", "KIND", "ON DEMAND", "STATE"];
    let width = |i: usize| rows.iter().map(|r| r[i].len()).chain([head[i].len()]).max().unwrap_or(0);
    let (w0, w1, w2) = (width(0), width(1), width(2));
    println!("{:w0$}  {:w1$}  {:w2$}  {}", head[0], head[1], head[2], head[3]);
    for r in rows {
        println!("{:w0$}  {:w1$}  {:w2$}  {}", r[0], r[1], r[2], r[3]);
    }
}

/// After a change: what it is now, as a sentence.
fn sentence(u: &Value) -> String {
    let name = u["name"].as_str().unwrap_or("");
    match u["after"].as_str() {
        Some(after) => format!("{name}: on demand, parked after {after} without use ({}); it's {} now", u["from"].as_str().unwrap_or("setting"), u["state"].as_str().unwrap_or("")),
        None => match (u["setting"].as_str(), u["label"].as_str()) {
            (Some("off"), Some(label)) => format!("{name}: not on demand (turned off here; its label says {label})"),
            (Some("off"), None) => format!("{name}: not on demand (turned off here)"),
            (_, Some(_)) if u["wakeable"] == false => format!("{name}: not on demand: nothing can wake it (publish a port)"),
            _ => format!("{name}: not on demand"),
        },
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn on_demand_reads_in_a_few_words() {
        let u = |v: serde_json::Value| super::on_demand(&v);
        assert_eq!(u(json!({"after": "15m", "from": "label"})), "after 15m (label)");
        assert_eq!(u(json!({"after": null, "setting": "off", "from": "setting"})), "off (setting)");
        assert_eq!(u(json!({"after": null, "label": "5m", "wakeable": false})), "off (nothing can wake it: publish a port)");
        assert_eq!(u(json!({"after": null})), "off");
    }

    #[test]
    fn a_change_reads_as_a_sentence() {
        let s = |v: serde_json::Value| super::sentence(&v);
        assert_eq!(s(json!({"name": "shop", "after": "15m", "from": "setting", "state": "running"})), "shop: on demand, parked after 15m without use (setting); it's running now");
        assert_eq!(s(json!({"name": "blog", "after": null, "setting": "off", "label": "5m"})), "blog: not on demand (turned off here; its label says 5m)");
        assert_eq!(s(json!({"name": "solo", "after": null, "setting": null, "label": null})), "solo: not on demand");
    }
}
