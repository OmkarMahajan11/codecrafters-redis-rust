#![allow(unused_imports)]
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    // Uncomment the code below to pass the first stage
    //
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut buf = [0; 1024];

                loop {
                    let n = match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("read error: {e}");
                            break;
                        }
                    };

                    if buf[..n].eq(b"*1\r\n$4\r\nPING\r\n") {
                        _ = stream.write_all(b"+PONG\r\n");
                    }
                }
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}
