use crate::message::{ClientMessage, client_message, ServerMessage};
use log::{error, info};
use prost::Message;
use std::io::{Read, Write};
use std::{
    io,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

pub struct Client {
    ip: String,
    port: u32,
    timeout: Duration,
    stream: Option<TcpStream>,
}

impl Client {
    pub fn new(ip: &str, port: u32, timeout_ms: u64) -> Self {
        Client {
            ip: ip.to_string(),
            port,
            timeout: Duration::from_millis(timeout_ms),
            stream: None,
        }
    }

    pub fn connect(&mut self) -> io::Result<()> {
        println!("Connecting to {}:{}", self.ip, self.port);

        let address = format!("{}:{}", self.ip, self.port);
        let socket_addrs: Vec<SocketAddr> = address.to_socket_addrs()?.collect();

        if socket_addrs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid IP or port",
            ));
        }

        let stream = TcpStream::connect_timeout(&socket_addrs[0], self.timeout)?;
        self.stream = Some(stream);

        println!("Connected to the server!");
        Ok(())
    }

    pub fn disconnect(&mut self) -> io::Result<()> {
        if let Some(stream) = self.stream.take() {
            stream.shutdown(std::net::Shutdown::Both)?;
        }

        println!("Disconnected from the server!");
        Ok(())
    }

    pub fn send(&mut self, message: client_message::Message) -> io::Result<()> {
        if let Some(ref mut stream) = self.stream {
            let client_message = ClientMessage {
                message: Some(message),
            };
            
            let payload = client_message.encode_to_vec();
            let len = payload.len() as u32;
            
            // Write length prefix
            stream.write_all(&len.to_be_bytes())?;
            
            // Write payload
            stream.write_all(&payload)?;
            stream.flush()?;

            println!("Sent message: {:?}", client_message);
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotConnected, "No active connection"))
        }
    }

    pub fn receive(&mut self) -> io::Result<ServerMessage> {
        if let Some(ref mut stream) = self.stream {
            info!("Receiving message from the server");
            
            // Read message length
            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf)?;
            let message_len = u32::from_be_bytes(len_buf) as usize;

            // Read the message
            let mut buffer = vec![0u8; message_len];
            stream.read_exact(&mut buffer)?;

            ServerMessage::decode(&buffer[..]).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Failed to decode ServerMessage: {}", e),
                )
            })
        } else {
            error!("No active connection");
            Err(io::Error::new(io::ErrorKind::NotConnected, "No active connection"))
        }
    }
}