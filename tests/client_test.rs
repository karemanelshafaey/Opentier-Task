use serial_test::serial;
use task::{
    message::{client_message, server_message, AddRequest, EchoMessage},
    server::Server,
    client::Client,
};
use std::{
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

fn setup_server_thread(server: Arc<Server>) -> JoinHandle<()> {
    let handle = thread::spawn(move || {
        server.run().expect("Server encountered an error");
    });
    thread::sleep(Duration::from_millis(200));
    handle
}

fn create_server() -> Arc<Server> {
    Arc::new(Server::new("localhost:8080").expect("Failed to start server"))
}

#[test]
#[serial]
fn test_client_connection() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    let mut client = Client::new("localhost", 8080, 2000);
    thread::sleep(Duration::from_millis(50));
    assert!(client.connect().is_ok(), "Failed to connect to the server");
    thread::sleep(Duration::from_millis(50));
    assert!(client.disconnect().is_ok(), "Failed to disconnect from the server");

    server.stop();
    assert!(handle.join().is_ok());
}

#[test]
#[serial]
fn test_client_echo_message() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    let mut client = Client::new("localhost", 8080, 2000);
    assert!(client.connect().is_ok());
    thread::sleep(Duration::from_millis(50));

    let mut echo_message = EchoMessage::default();
    echo_message.content = "Hello, World!".to_string();
    let message = client_message::Message::EchoMessage(echo_message.clone());

    assert!(client.send(message).is_ok());
    thread::sleep(Duration::from_millis(50));

    let response = client.receive();
    assert!(response.is_ok());

    match response.unwrap().message {
        Some(server_message::Message::EchoMessage(echo)) => {
            assert_eq!(echo.content, echo_message.content);
        }
        _ => panic!("Expected EchoMessage"),
    }

    thread::sleep(Duration::from_millis(50));
    assert!(client.disconnect().is_ok());
    server.stop();
    handle.join().unwrap();
}

#[test]
#[serial]
fn test_client_add_request() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    let mut client = Client::new("localhost", 8080, 2000);
    assert!(client.connect().is_ok());
    thread::sleep(Duration::from_millis(50));

    let add_request = AddRequest { a: 10, b: 20 };
    let message = client_message::Message::AddRequest(add_request);

    assert!(client.send(message).is_ok());
    thread::sleep(Duration::from_millis(50));

    let response = client.receive();
    assert!(response.is_ok());

    match response.unwrap().message {
        Some(server_message::Message::AddResponse(add_response)) => {
            assert_eq!(add_response.result, 30);
        }
        _ => panic!("Expected AddResponse"),
    }

    thread::sleep(Duration::from_millis(50));
    assert!(client.disconnect().is_ok());
    server.stop();
    handle.join().unwrap();
}

#[test]
#[serial]
fn test_server_scalability() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    let client_counts = vec![5, 10, 20]; // Reduced counts for testing
    
    for &num_clients in &client_counts {
        let mut handles = vec![];
        let start_time = Instant::now();
        
        for i in 0..num_clients {
            let handle = thread::spawn(move || {
                let mut client = Client::new("localhost", 8080, 2000);
                thread::sleep(Duration::from_millis(50));
                assert!(client.connect().is_ok());

                for j in 0..3 { // Reduced requests for testing
                    thread::sleep(Duration::from_millis(50));
                    let message = if j % 2 == 0 {
                        client_message::Message::EchoMessage(EchoMessage {
                            content: format!("Client {} message {}", i, j),
                        })
                    } else {
                        client_message::Message::AddRequest(AddRequest { 
                            a: i as i32, 
                            b: j as i32 
                        })
                    };

                    assert!(client.send(message).is_ok());
                    thread::sleep(Duration::from_millis(50));
                    assert!(client.receive().is_ok());
                }

                thread::sleep(Duration::from_millis(50));
                client.disconnect().unwrap();
            });
            handles.push(handle);
            thread::sleep(Duration::from_millis(20));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let duration = start_time.elapsed();
        println!(
            "Completed {} concurrent clients in {:?}",
            num_clients, duration
        );
    }

    server.stop();
    handle.join().unwrap();
}

#[test]
#[serial]
fn test_concurrent_request_handling() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    const NUM_CLIENTS: usize = 5; // Reduced for testing
    const REQUESTS_PER_CLIENT: usize = 10;
    
    let start_time = Instant::now();
    let mut handles = vec![];
    let success_count = Arc::new(AtomicUsize::new(0));

    for client_id in 0..NUM_CLIENTS {
        let success_counter = Arc::clone(&success_count);
        
        let handle = thread::spawn(move || {
            let mut client = Client::new("localhost", 8080, 2000);
            thread::sleep(Duration::from_millis(50));
            if client.connect().is_ok() {
                let mut successful_requests = 0;

                for req_id in 0..REQUESTS_PER_CLIENT {
                    thread::sleep(Duration::from_millis(50));
                    let message = if req_id % 2 == 0 {
                        client_message::Message::EchoMessage(EchoMessage {
                            content: format!("Client {} Request {}", client_id, req_id),
                        })
                    } else {
                        client_message::Message::AddRequest(AddRequest {
                            a: client_id as i32,
                            b: req_id as i32,
                        })
                    };

                    if client.send(message).is_ok() {
                        thread::sleep(Duration::from_millis(50));
                        if client.receive().is_ok() {
                            successful_requests += 1;
                        }
                    }
                }

                success_counter.fetch_add(successful_requests, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(50));
                client.disconnect().unwrap();
            }
        });
        handles.push(handle);
        thread::sleep(Duration::from_millis(20));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let duration = start_time.elapsed();
    let total_successful_requests = success_count.load(Ordering::SeqCst);
    let total_requests = NUM_CLIENTS * REQUESTS_PER_CLIENT;
    
    println!("Total time: {:?}", duration);
    println!("Successful requests: {}/{}", total_successful_requests, total_requests);
    println!("Requests per second: {}", total_successful_requests as f64 / duration.as_secs_f64());

    assert!(total_successful_requests > 0, "No successful requests");
    assert!(
        total_successful_requests as f64 / total_requests as f64 >= 0.95,
        "Success rate below 95%"
    );

    server.stop();
    handle.join().unwrap();
}

#[test]
#[serial]
fn test_connection_timeout() {
    let mut client = Client::new("192.0.2.1", 8080, 100);
    assert!(client.connect().is_err(), "Should timeout quickly");
}

#[test]
#[serial]
fn test_message_order_preservation() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    let mut client = Client::new("localhost", 8080, 2000);
    assert!(client.connect().is_ok());
    thread::sleep(Duration::from_millis(50));

    let num_messages = 5; // Reduced for testing
    
    for i in 0..num_messages {
        thread::sleep(Duration::from_millis(50));
        let message = client_message::Message::EchoMessage(EchoMessage {
            content: format!("Message {}", i),
        });
        assert!(client.send(message).is_ok());
    }

    for i in 0..num_messages {
        thread::sleep(Duration::from_millis(50));
        let response = client.receive().unwrap();
        match response.message.unwrap() {
            server_message::Message::EchoMessage(echo) => {
                assert_eq!(echo.content, format!("Message {}", i));
            }
            _ => panic!("Unexpected message type"),
        }
    }

    thread::sleep(Duration::from_millis(50));
    assert!(client.disconnect().is_ok());
    server.stop();
    handle.join().unwrap();
}

#[test]
#[serial]
fn test_large_message_handling() {
    let server = create_server();
    let handle = setup_server_thread(server.clone());

    let mut client = Client::new("localhost", 8080, 2000);
    assert!(client.connect().is_ok());
    thread::sleep(Duration::from_millis(50));

    let large_content = "x".repeat(10_000);
    let message = client_message::Message::EchoMessage(EchoMessage {
        content: large_content.clone(),
    });

    assert!(client.send(message).is_ok());
    thread::sleep(Duration::from_millis(50));
    
    let response = client.receive().unwrap();
    match response.message.unwrap() {
        server_message::Message::EchoMessage(echo) => {
            assert_eq!(echo.content, large_content);
        }
        _ => panic!("Unexpected message type"),
    }

    thread::sleep(Duration::from_millis(50));
    assert!(client.disconnect().is_ok());
    server.stop();
    handle.join().unwrap();
}