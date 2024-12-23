use task::server::Server;
use log::error;
use std::sync::Arc;

fn main() {
    env_logger::init();

    match Server::new("127.0.0.1:8080") {
        Ok(server) => {
            // Wrap server in an Arc to share ownership with the handler
            let server = Arc::new(server);
            let server_clone = Arc::clone(&server);
            
            ctrlc::set_handler(move || {
                server_clone.stop();
            })
            .expect("Error setting Ctrl-C handler");

            if let Err(e) = server.run() {
                error!("Server error: {}", e);
            }
        }
        Err(e) => error!("Failed to create server: {}", e),
    }
}