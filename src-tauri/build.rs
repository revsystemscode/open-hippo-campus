fn main() {
    // Tauri incrusta los iconos al compilar, pero Cargo no sabe que dependen
    // de estos archivos: sin esto, cambiar un icono no recompila la app.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
