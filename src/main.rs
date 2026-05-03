#![allow(unused_imports)]
use core::time;
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    ops::Deref,
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;

use calloop::{EventLoop, Interest, LoopHandle, Mode, PostAction, generic::Generic};

mod commands;
use commands::{Command, Entry};

struct ServerData {
    next_id: usize,
    handle: LoopHandle<'static, ServerData>,
    store: HashMap<String, Entry>,
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
        store: HashMap::new(),
    };

    event_loop.run(None, &mut shared, |_data| {})?;

    Ok(())
}

fn register_client(handle: LoopHandle<'static, ServerData>, stream: TcpStream) {
    _ = handle.insert_source(
        Generic::new(stream, Interest::READ, Mode::Edge),
        |_readiness, stream, data: &mut ServerData| {
            let mut buf = [0; 1024];
            let mut tcp: &TcpStream = stream.deref();

            let n = match tcp.read(&mut buf) {
                Ok(0) => return Ok(PostAction::Remove),
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(PostAction::Continue),
                Err(_) => return Ok(PostAction::Remove),
            };

            commands::handle_command(
                commands::parse_command(&buf[..n]),
                &mut data.store,
                &mut tcp,
            );

            Ok(PostAction::Continue)
        },
    );
}
