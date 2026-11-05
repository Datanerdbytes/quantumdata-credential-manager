mod cli;
mod config;
mod crypto;
mod env_import;
mod models;
mod notion;
mod rotation;
mod secrets;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("Application error: {error}");
    }
}
