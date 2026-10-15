//! Engine API inspect responses → `RawContainer`. Environment values and every label
//! outside `com.docker.compose.*` are dropped here.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

use bollard::models::ContainerInspectResponse;

use hm_core::model::{Mount, PortBinding};
use hm_core::raw::RawContainer;

const COMPOSE_LABEL_PREFIX: &str = "com.docker.compose.";
const NEVER_STARTED: &str = "0001-01-01T00:00:00Z";

pub fn from_inspect(engine: &str, inspect: ContainerInspectResponse) -> Option<RawContainer> {
    let id = inspect.id?;
    let state = inspect.state.unwrap_or_default();
    let config = inspect.config.unwrap_or_default();
    let network = inspect.network_settings.unwrap_or_default();

    let mut ports: Vec<PortBinding> = Vec::new();
    for (key, bindings) in network.ports.unwrap_or_default() {
        let Some((port, proto)) = key.split_once('/') else {
            continue;
        };
        let Ok(container_port) = port.parse::<u16>() else {
            continue;
        };
        let bindings = bindings.unwrap_or_default();
        if bindings.is_empty() {
            ports.push(PortBinding {
                container_port,
                proto: proto.to_string(),
                host_ip: None,
                host_port: None,
            });
        }
        for binding in bindings {
            ports.push(PortBinding {
                container_port,
                proto: proto.to_string(),
                host_ip: binding
                    .host_ip
                    .as_deref()
                    .filter(|ip| !ip.is_empty())
                    .and_then(|ip| ip.parse::<IpAddr>().ok()),
                host_port: binding.host_port.as_deref().and_then(|p| p.parse().ok()),
            });
        }
    }
    ports.sort_by(|a, b| {
        (a.container_port, &a.proto, a.host_port, a.host_ip).cmp(&(
            b.container_port,
            &b.proto,
            b.host_port,
            b.host_ip,
        ))
    });

    let mut networks: Vec<String> = network.networks.unwrap_or_default().into_keys().collect();
    networks.sort();

    let mut mounts: Vec<Mount> = inspect
        .mounts
        .unwrap_or_default()
        .into_iter()
        .map(|mount| {
            let kind = mount.typ.unwrap_or_default();
            let source = if kind == "bind" {
                mount.source.map(PathBuf::from)
            } else {
                None
            };
            Mount {
                kind,
                name: mount.name.filter(|name| !name.is_empty()),
                source,
                destination: mount.destination.unwrap_or_default(),
            }
        })
        .collect();
    // Docker lists mounts in no fixed order; sort them so snapshots stay stable.
    mounts.sort_by(|a, b| {
        (&a.destination, &a.kind, &a.name, &a.source).cmp(&(
            &b.destination,
            &b.kind,
            &b.name,
            &b.source,
        ))
    });

    let labels: BTreeMap<String, String> = config
        .labels
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| key.starts_with(COMPOSE_LABEL_PREFIX))
        .collect();

    Some(RawContainer {
        engine: engine.to_string(),
        id,
        name: inspect
            .name
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        image: config.image.or(inspect.image).unwrap_or_default(),
        state: state
            .status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "unknown".into()),
        health: state
            .health
            .and_then(|health| health.status)
            .map(|status| status.to_string())
            .filter(|status| !status.is_empty() && status != "none"),
        exit_code: state.exit_code,
        oom_killed: state.oom_killed.unwrap_or(false),
        restart_count: inspect.restart_count.unwrap_or(0),
        started_at: state
            .started_at
            .filter(|at| !at.is_empty() && at != NEVER_STARTED),
        ports,
        networks,
        mounts,
        labels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSTGRES: &str = include_str!("../../tests/fixtures/inspect-compose-postgres.json");

    #[test]
    fn converts_a_compose_postgres_container() {
        let inspect: ContainerInspectResponse = serde_json::from_str(POSTGRES).unwrap();
        let container = from_inspect("e_engine", inspect).unwrap();
        assert_eq!(container.name, "shop-db-1");
        assert_eq!(container.image, "postgres:16-alpine");
        assert_eq!(container.state, "running");
        assert_eq!(container.health.as_deref(), Some("healthy"));
        assert_eq!(container.restart_count, 0);
        assert_eq!(
            container.started_at.as_deref(),
            Some("2026-10-15T07:00:03.123456789Z")
        );
        assert_eq!(
            container.ports,
            vec![
                PortBinding {
                    container_port: 5432,
                    proto: "tcp".into(),
                    host_ip: Some("0.0.0.0".parse().unwrap()),
                    host_port: Some(55432),
                },
                PortBinding {
                    container_port: 5432,
                    proto: "tcp".into(),
                    host_ip: Some("::".parse().unwrap()),
                    host_port: Some(55432),
                },
            ]
        );
        assert_eq!(container.networks, vec!["shop_default"]);
        // Sorted by destination; a bind mount keeps its host path, a volume its name.
        assert_eq!(container.mounts[0].kind, "bind");
        assert_eq!(
            container.mounts[0].source,
            Some(PathBuf::from("/Users/me/code/shop/db/init"))
        );
        assert_eq!(container.mounts[1].kind, "volume");
        assert_eq!(container.mounts[1].name.as_deref(), Some("shop_pgdata"));
        assert_eq!(container.mounts[1].source, None);
        assert_eq!(
            container
                .labels
                .get("com.docker.compose.service")
                .map(String::as_str),
            Some("db")
        );
        assert!(
            !container
                .labels
                .contains_key("org.opencontainers.image.version")
        );
        let json = serde_json::to_string(&container).unwrap();
        assert!(!json.contains("POSTGRES_PASSWORD"));
        assert!(!json.contains("example-secret"));
    }
}
