//! Prueba de humo contra un motor Docker real.
//!
//! Crea Redis y PostgreSQL desde el catálogo, espera a que estén sanos,
//! comprueba que responden y los borra (con sus datos).
//!
//!     cargo run --example smoke

use std::time::{Duration, Instant};

use open_hippo_campus_lib::{engine::Engine, services};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let engine = Engine::default();
    let status = engine.status().await;
    assert!(status.connected, "sin motor: {:?}", status.error);
    println!(
        "Motor: {} · Docker {} · {}",
        status.provider.unwrap_or_default(),
        status.server_version.unwrap_or_default(),
        status.endpoint.unwrap_or_default()
    );
    let docker = engine.connection().await.unwrap().docker;

    let report = |p: services::Progress| {
        if p.phase != "pull" || p.percent.is_some_and(|x| x as i64 % 25 == 0) {
            println!("  [{}] {:<6} {}", p.service_id, p.phase, p.message);
        }
    };

    for id in ["redis", "postgres"] {
        println!("\n== Creando {id}");
        let created = services::create(&report, &docker, id).await.expect("crear servicio");
        println!("  conexión: {}", created.connection);

        // Espera a que el healthcheck dé "healthy".
        let start = Instant::now();
        let view = loop {
            let list = services::list(&docker).await.unwrap();
            let v = list.into_iter().find(|c| c.id == created.id).expect("contenedor en la lista");
            if v.health.as_deref() == Some("healthy") {
                break v;
            }
            assert!(start.elapsed() < Duration::from_secs(90), "no llegó a healthy: {:?}", v.health);
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        println!(
            "  healthy en {:.1}s · estado={} · puertos={:?}",
            start.elapsed().as_secs_f64(),
            view.state,
            view.ports.iter().map(|p| (p.private_port, p.public_port)).collect::<Vec<_>>()
        );
        assert!(view.managed);
        assert_eq!(view.service_id.as_deref(), Some(id));

        // Conexión real desde el host al puerto publicado.
        let host_port = view.ports[0].public_port.expect("puerto publicado");
        let mut sock = std::net::TcpStream::connect(("127.0.0.1", host_port)).expect("conectar al puerto del host");
        if id == "redis" {
            use std::io::{Read, Write};
            let password = created.connection.split(':').nth(2).unwrap().split('@').next().unwrap();
            write!(sock, "AUTH {password}\r\nPING\r\n").unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut buf = [0u8; 64];
            let n = sock.read(&mut buf).unwrap();
            let reply = String::from_utf8_lossy(&buf[..n]).replace("\r\n", " ");
            assert!(reply.contains("+OK"), "AUTH falló: {reply}");
            println!("  AUTH + PING desde el host: {reply}");
        } else {
            println!("  TCP desde el host a 127.0.0.1:{host_port}: OK");
        }

        let logs = services::logs(&docker, &created.id, 5).await.unwrap();
        println!("  logs (últimas líneas): {}", logs.lines().last().unwrap_or(""));

        services::stop(&docker, &created.id).await.unwrap();
        services::start(&docker, &created.id).await.unwrap();
        println!("  stop/start OK");

        services::remove(&docker, &created.id, true).await.unwrap();
        let still = services::list(&docker).await.unwrap().into_iter().any(|c| c.id == created.id);
        assert!(!still, "el contenedor sigue existiendo");
        println!("  borrado (con volumen) OK");
    }
    println!("\nPrueba de humo completada.");
}
