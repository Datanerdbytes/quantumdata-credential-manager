mod cli;
mod config;
mod models;
mod notion;
mod onepassword;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("Application error: {error}");
    }
}
