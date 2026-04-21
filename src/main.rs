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

struct Entry {
    value: String,
    expiry: Option<Instant>,
}

struct ServerData {
    next_id: usize,
    handle: LoopHandle<'static, ServerData>,
    store: HashMap<String, Entry>,
}

const ECHO_PREFIX: &[u8; 14] = b"*2\r\n$4\r\nECHO\r\n";
const SET_PREFIX: &[u8; 13] = b"*3\r\n$3\r\nSET\r\n";
const SET_EXP_PREFIX: &[u8; 13] = b"*5\r\n$3\r\nSET\r\n";
const GET_PREFIX: &[u8; 13] = b"*2\r\n$3\r\nGET\r\n";
const PING_PREFIX: &[u8; 14] = b"*1\r\n$4\r\nPING\r\n";

const PONG_RESPONSE: &[u8; 7] = b"+PONG\r\n";
const NULL_RESPONSE: &[u8; 5] = b"$-1\r\n";
const OK_RESPONSE: &[u8; 5] = b"+OK\r\n";

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

            if buf.starts_with(SET_PREFIX) {
                match parse_kv(&buf[SET_PREFIX.len()..n]) {
                    Some((key, value)) => {
                        data.store.insert(
                            key,
                            Entry {
                                value,
                                expiry: None,
                            },
                        );
                        _ = tcp.write_all(OK_RESPONSE);
                    }
                    None => _ = tcp.write_all(NULL_RESPONSE),
                }
            } else if buf.starts_with(SET_EXP_PREFIX) {
                match parse_kve(&buf[SET_EXP_PREFIX.len()..n]) {
                    Some((key, value, expiry)) => {
                        data.store.insert(
                            key,
                            Entry {
                                value,
                                expiry: Instant::now()
                                    .checked_add(time::Duration::from_millis(expiry)),
                            },
                        );
                        _ = tcp.write_all(OK_RESPONSE);
                    }
                    None => _ = tcp.write_all(NULL_RESPONSE),
                }
            } else if buf.starts_with(GET_PREFIX) {
                match parse_v(&buf[GET_PREFIX.len()..n]) {
                    Some(key) => {
                        let value = data.store.get(&key);
                        match value {
                            Some(v) => {
                                if let Some(e) = v.expiry
                                    && Instant::now() > e
                                {
                                    data.store.remove(&key);
                                    _ = tcp.write_all(NULL_RESPONSE);
                                } else {
                                    _ = tcp.write_all(
                                        format!("${}\r\n{}\r\n", v.value.len(), v.value).as_bytes(),
                                    )
                                }
                            }
                            None => _ = tcp.write_all(NULL_RESPONSE),
                        }
                    }
                    None => _ = tcp.write_all(NULL_RESPONSE),
                }
            } else if buf.starts_with(ECHO_PREFIX) {
                _ = tcp.write_all(&buf[ECHO_PREFIX.len()..n])
            } else if buf.starts_with(PING_PREFIX) {
                _ = tcp.write_all(PONG_RESPONSE);
            } else {
                _ = tcp.write_all(NULL_RESPONSE);
            }

            Ok(PostAction::Continue)
        },
    );
}

// *3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n

// input: $3\r\nkey\r\n$5\r\nvalue\r\n
fn parse_kv(buf: &[u8]) -> Option<(String, String)> {
    // buf: $3\r\nkey\r\n$5\r\nvalue\r\n
    let s = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .collect::<Vec<&str>>();

    Some((s.get(1)?.to_string(), s.get(3)?.to_string()))
}

// input: $3\r\nkey\r\n
fn parse_v(buf: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .collect::<Vec<&str>>();

    Some(s.get(1)?.to_string())
}

// input: $3\r\nkey\r\n$5\r\nvalue\r\n$2\r\npx\r\n$3\r\n100\r\n
fn parse_kve(buf: &[u8]) -> Option<(String, String, u64)> {
    let s = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .collect::<Vec<&str>>();

    let key = s.get(1)?.to_string();
    let value = s.get(3)?.to_string();
    let mut expiry = s.get(7)?.parse::<u64>().ok()?;

    if s.get(5)?.to_string().to_lowercase() == "ex" {
        expiry *= 1000;
    }

    Some((key, value, expiry))
}
