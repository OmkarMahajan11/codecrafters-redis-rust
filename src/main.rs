#![allow(unused_imports)]
use std::{
    io::{Read, Write},
    thread,
};

use async_net::TcpListener;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};

use smol::io;

fn main() -> io::Result<()> {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    smol::block_on(async {
        let listener = TcpListener::bind("127.0.0.1:6379").await?;

        loop {
            let (mut stream, _addr) = listener.accept().await?;

            let _ = smol::spawn(async move {
                let mut buf = [0; 1024];

                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(0) => return,
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("read error: {e}");
                            return;
                        }
                    };

                    if let Err(e) = stream.write_all(b"+PONG\r\n").await {
                        eprintln!("write error: {e}");
                        return;
                    }
                }
            })
            .detach();
        }
    })
}
