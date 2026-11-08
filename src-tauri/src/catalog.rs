//! Catálogo de servicios listos para crear con un clic.
//!
//! Cada servicio es una plantilla declarativa (ver `catalog.json`). Los valores
//! pueden usar marcadores que se rellenan al crear la instancia:
//!   - `{{password}}`        contraseña aleatoria generada para esa instancia
//!   - `{{port.<interno>}}`  puerto del host asignado a ese puerto del contenedor

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePort {
    pub container: u16,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogService {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub image: String,
    pub ports: Vec<ServicePort>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub cmd: Option<Vec<String>>,
    #[serde(default)]
    pub data_path: Option<String>,
    #[serde(default)]
    pub healthcheck: Option<Vec<String>>,
    pub connection: String,
    #[serde(default)]
    pub web_ui: Option<String>,
}

const CATALOG_JSON: &str = include_str!("../catalog.json");

pub fn all() -> Vec<CatalogService> {
    serde_json::from_str(CATALOG_JSON).expect("catalog.json no es válido")
}

pub fn find(id: &str) -> Option<CatalogService> {
    all().into_iter().find(|s| s.id == id)
}

/// Valores concretos de una instancia: contraseña y puertos del host.
pub struct Values {
    pub password: String,
    /// puerto del contenedor -> puerto del host
    pub ports: HashMap<u16, u16>,
}

impl Values {
    pub fn render(&self, template: &str) -> String {
        let mut out = template.replace("{{password}}", &self.password);
        for (container, host) in &self.ports {
            out = out.replace(&format!("{{{{port.{container}}}}}"), &host.to_string());
        }
        out
    }

    pub fn render_all(&self, items: &[String]) -> Vec<String> {
        items.iter().map(|s| self.render(s)).collect()
    }
}

/// Contraseña aleatoria sin caracteres ambiguos ni símbolos que rompan URLs.
///
/// Usa `RandomState`, que se inicializa con entropía del sistema operativo.
/// Es suficiente para bases de datos de desarrollo locales.
pub fn random_password(len: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let base = CHARS.len() as u64;
    let mut out = String::with_capacity(len);
    let mut round = 0u64;
    while out.len() < len {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(round);
        let mut v = h.finish();
        for _ in 0..8 {
            if out.len() >= len {
                break;
            }
            out.push(CHARS[(v % base) as usize] as char);
            v /= base;
        }
        round += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_ids_are_unique() {
        let items = all();
        assert!(items.len() >= 2);
        let mut ids: Vec<_> = items.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), items.len());
    }

    #[test]
    fn every_port_placeholder_matches_a_declared_port() {
        for s in all() {
            let values = Values {
                password: "x".into(),
                ports: s.ports.iter().map(|p| (p.container, p.container + 1)).collect(),
            };
            let mut texts = vec![s.connection.clone()];
            texts.extend(s.web_ui.clone());
            texts.extend(s.env.values().cloned());
            texts.extend(s.cmd.clone().unwrap_or_default());
            texts.extend(s.healthcheck.clone().unwrap_or_default());
            for t in texts {
                let r = values.render(&t);
                assert!(!r.contains("{{"), "marcador sin resolver en {}: {r}", s.id);
            }
        }
    }

    #[test]
    fn render_replaces_password_and_ports() {
        let v = Values {
            password: "secreto".into(),
            ports: HashMap::from([(5432, 5433)]),
        };
        assert_eq!(
            v.render("postgres://dev:{{password}}@localhost:{{port.5432}}/app"),
            "postgres://dev:secreto@localhost:5433/app"
        );
    }

    #[test]
    fn password_has_requested_length_and_varies() {
        let a = random_password(20);
        let b = random_password(20);
        assert_eq!(a.len(), 20);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, b);
    }
}
