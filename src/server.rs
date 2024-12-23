use crate::message::{ClientMessage, ServerMessage, EchoMessage, AddRequest, AddResponse};
use crate::message::client_message::Message as ClientMessageEnum;
use crate::message::server_message::Message as ServerMessageEnum;
use log::{error, info, warn};
use prost::Message;
use std::{
    io::{self, ErrorKind, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const MAX_MESSAGE_SIZE: usize = 1024 * 1024; 
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const THREAD_POOL_SIZE: usize = 4;

struct ThreadPool {
    workers: Vec<Worker>,
    sender: crossbeam_channel::Sender<ThreadPoolMessage>,
}

struct Worker {
    thread: Option<JoinHandle<()>>,
}

enum ThreadPoolMessage {
    NewJob(Job),
    Terminate,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ThreadPool {
    pub fn new(size: usize) -> ThreadPool {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let receiver = Arc::new(Mutex::new(receiver));
        
        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.send(ThreadPoolMessage::NewJob(job)).unwrap();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        for _ in &self.workers {
            self.sender.send(ThreadPoolMessage::Terminate).unwrap();
        }

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<crossbeam_channel::Receiver<ThreadPoolMessage>>>) -> Worker {
        let thread = thread::spawn(move || loop {
            let message = receiver.lock().unwrap().recv().unwrap();
            
            match message {
                ThreadPoolMessage::NewJob(job) => {
                    info!("Worker {} got a job; executing.", id);
                    job();
                }
                ThreadPoolMessage::Terminate => {
                    info!("Worker {} was told to terminate.", id);
                    break;
                }
            }
        });

        Worker {
            thread: Some(thread),
        }
    }
}

struct Client {
    stream: TcpStream,
}

impl Client {
    pub fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_nodelay(true)?;
        Ok(Client { stream })
    }

    fn read_message(&mut self) -> io::Result<Vec<u8>> {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf)?;
        
        let message_len = u32::from_be_bytes(len_buf) as usize;
        if message_len > MAX_MESSAGE_SIZE {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "Message size exceeds maximum allowed",
            ));
        }

        let mut buffer = vec![0; message_len];
        self.stream.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    fn write_message(&mut self, payload: &[u8]) -> io::Result<()> {
        let len = payload.len() as u32;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(payload)?;
        self.stream.flush()
    }

    pub fn handle(&mut self) -> io::Result<bool> {
        self.stream.set_nonblocking(false)?;
        match self.read_message() {
            Ok(buffer) => {
                match ClientMessage::decode(&buffer[..]) {
                    Ok(client_msg) => {
                        if let Some(message) = client_msg.message {
                            let response = match message {
                                ClientMessageEnum::EchoMessage(echo) => {
                                    info!("Handling echo message: {}", echo.content);
                                    self.handle_echo(echo)
                                }
                                ClientMessageEnum::AddRequest(add) => {
                                    info!("Handling add request: {} + {}", add.a, add.b);
                                    self.handle_add(add)
                                }
                            }?;
                            
                            let encoded = response.encode_to_vec();
                            self.write_message(&encoded)?;
                            Ok(true)
                        } else {
                            warn!("Received empty message");
                            Ok(true)
                        }
                    }
                    Err(e) => {
                        error!("Failed to decode message: {}", e);
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                if e.kind() == ErrorKind::UnexpectedEof {
                    Ok(false)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn handle_echo(&mut self, msg: EchoMessage) -> io::Result<ServerMessage> {
        Ok(ServerMessage {
            message: Some(ServerMessageEnum::EchoMessage(msg))
        })
    }

    fn handle_add(&mut self, req: AddRequest) -> io::Result<ServerMessage> {
        let result = req.a + req.b;
        Ok(ServerMessage {
            message: Some(ServerMessageEnum::AddResponse(AddResponse {
                result,
            }))
        })
    }
}

pub struct Server {
    listener: TcpListener,
    is_running: Arc<AtomicBool>,
    thread_pool: ThreadPool,
}

impl Server {
    pub fn new(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        
        Ok(Server {
            listener,
            is_running: Arc::new(AtomicBool::new(false)),
            thread_pool: ThreadPool::new(THREAD_POOL_SIZE),
        })
    }

    pub fn run(&self) -> io::Result<()> {
        self.is_running.store(true, Ordering::SeqCst);
        info!("Server running on {}", self.listener.local_addr()?);

        while self.is_running.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, addr)) => {
                    info!("New client connected: {}", addr);
                    let is_running = Arc::clone(&self.is_running);
                    
                    self.thread_pool.execute(move || {
                        if let Ok(mut client) = Client::new(stream) {
                            while is_running.load(Ordering::SeqCst) {
                                match client.handle() {
                                    Ok(true) => continue,
                                    Ok(false) => break,
                                    Err(e) => {
                                        error!("Error handling client: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        info!("Client {} disconnected", addr);
                    });
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                    break;
                }
            }
        }

        info!("Server stopped");
        Ok(())
    }

    pub fn stop(&self) {
        if self.is_running.load(Ordering::SeqCst) {
            self.is_running.store(false, Ordering::SeqCst);
            // Connect to self to unblock accept
            if let Ok(addr) = self.listener.local_addr() {
                let _ = TcpStream::connect(addr);
            }
            info!("Shutdown signal sent");
        } else {
            warn!("Server already stopped or not running");
        }
    }
}