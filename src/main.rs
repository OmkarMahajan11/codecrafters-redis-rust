#![allow(unused_imports)]
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    ops::Deref,
    thread,
};

use anyhow::Result;

use calloop::{EventLoop, Interest, LoopHandle, Mode, PostAction, generic::Generic};

struct ServerData {
    next_id: usize,
    handle: LoopHandle<'static, ServerData>,
}

fn main() -> Result<()> {
    let mut event_loop: EventLoop<'static, ServerData> = EventLoop::try_new()?;
    let handle = event_loop.handle();

    let listener = TcpListener::bind("127.0.0.1:6379")?;
    listener.set_nonblocking(true)?;

    _ = handle.insert_source(
        Generic::new(listener, Interest::READ, Mode::Edge),
        |_readiness, listener, data: &mut ServerData| {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(true)?;
                        data.next_id += 1;
                        register_client(data.handle.clone(), stream);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }

            Ok(PostAction::Continue)
        },
    );

    let mut shared = ServerData {
        next_id: 0,
        handle: handle.clone(),
    };

    event_loop.run(None, &mut shared, |_data| {})?;

    Ok(())
}

fn register_client(handle: LoopHandle<'static, ServerData>, stream: TcpStream) {
    _ = handle.insert_source(
        Generic::new(stream, Interest::READ, Mode::Edge),
        |_readiness, stream, _data: &mut ServerData| {
            let mut buf = [0; 1024];
            let mut tcp: &TcpStream = stream.deref();

            let n = match tcp.read(&mut buf) {
                Ok(0) => return Ok(PostAction::Remove),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(PostAction::Continue),
                Err(_) => return Ok(PostAction::Remove),
            };

            let echo = b"*2\r\n$4\r\nECHO\r\n";
            if buf.starts_with(echo) {
                _ = tcp.write_all(&buf[echo.len()..n])
            } else {
                _ = tcp.write_all(b"+PONG\r\n");
            }

            Ok(PostAction::Continue)
        },
    );
}
