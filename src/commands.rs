use std::{
    collections::HashMap,
    io::Write,
    net::TcpStream,
    time::{Duration, Instant},
};

const ECHO_PREFIX: &[u8; 14] = b"*2\r\n$4\r\nECHO\r\n";
const SET_PREFIX: &[u8; 13] = b"*3\r\n$3\r\nSET\r\n";
const SET_EXP_PREFIX: &[u8; 13] = b"*5\r\n$3\r\nSET\r\n";
const GET_PREFIX: &[u8; 13] = b"*2\r\n$3\r\nGET\r\n";
const PING_PREFIX: &[u8; 14] = b"*1\r\n$4\r\nPING\r\n";

const PONG_RESPONSE: &[u8; 7] = b"+PONG\r\n";
const NULL_RESPONSE: &[u8; 5] = b"$-1\r\n";
const OK_RESPONSE: &[u8; 5] = b"+OK\r\n";

pub enum Entry {
    Single {
        value: String,
        expiry: Option<Instant>,
    },
    List(Vec<String>),
}

pub enum Command {
    Ping,
    Echo(String),
    Get(String),
    Set {
        key: String,
        value: String,
    },
    SetWithExpiry {
        key: String,
        value: String,
        ttl: Duration,
    },
    RPush {
        key: String,
        values: Vec<String>,
    },
}

fn parse_kv(buf: &[u8]) -> Option<(String, String)> {
    let s = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .collect::<Vec<&str>>();

    let k = s.get(1)?.to_string();
    let v = s.get(3)?.to_string();

    println!("{} - {}", k, v);

    Some((s.get(1)?.to_string(), s.get(3)?.to_string()))
}

fn parse_v(buf: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .collect::<Vec<&str>>();

    Some(s.get(1)?.to_string())
}

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

fn parse_kvlist(buf: &[u8]) -> Option<(String, Vec<String>)> {
    let mut l: Vec<String> = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .filter(|x| !x.is_empty())
        .filter(|x| !x.starts_with("$"))
        .filter(|x| !x.starts_with("*"))
        .filter(|x| *x != "RPUSH")
        .map(|x| x.to_owned())
        .collect();

    if l.is_empty() {
        return None;
    }

    Some((l.swap_remove(0), l))
}

pub fn parse_command(buf: &[u8]) -> Option<Command> {
    if buf.starts_with(PING_PREFIX) {
        Some(Command::Ping)
    } else if buf.starts_with(ECHO_PREFIX) {
        parse_v(&buf[ECHO_PREFIX.len()..]).map(Command::Echo)
    } else if buf.starts_with(SET_PREFIX) {
        parse_kv(&buf[SET_PREFIX.len()..]).map(|(k, v)| Command::Set { key: k, value: v })
    } else if buf.starts_with(SET_EXP_PREFIX) {
        parse_kve(&buf[SET_EXP_PREFIX.len()..]).map(|(k, v, ttl)| Command::SetWithExpiry {
            key: k,
            value: v,
            ttl: Duration::from_millis(ttl),
        })
    } else if buf.starts_with(GET_PREFIX) {
        parse_v(&buf[GET_PREFIX.len()..]).map(Command::Get)
    } else if buf.windows(b"RPUSH".len()).any(|w| w == b"RPUSH") {
        parse_kvlist(buf).map(|(k, vl)| Command::RPush { key: k, values: vl })
    } else {
        None
    }
}

pub fn handle_command(
    command: Option<Command>,
    store: &mut HashMap<String, Entry>,
    mut tcp: &TcpStream,
) {
    match command {
        Some(Command::Set { key, value }) => {
            store.insert(
                key,
                Entry::Single {
                    value,
                    expiry: None,
                },
            );
            _ = tcp.write_all(OK_RESPONSE);
        }
        Some(Command::SetWithExpiry { key, value, ttl }) => {
            store.insert(
                key,
                Entry::Single {
                    value,
                    expiry: Instant::now().checked_add(ttl),
                },
            );
            _ = tcp.write_all(OK_RESPONSE);
        }
        Some(Command::Get(key)) => {
            let value = store.get(&key);
            match value {
                Some(Entry::Single { value, expiry }) => {
                    if let Some(e) = expiry
                        && Instant::now() > *e
                    {
                        store.remove(&key);
                        _ = tcp.write_all(NULL_RESPONSE);
                    } else {
                        _ = tcp.write_all(format!("${}\r\n{}\r\n", value.len(), value).as_bytes())
                    }
                }
                Some(Entry::List(_)) => {
                    _ = tcp.write_all(NULL_RESPONSE);
                }
                None => _ = tcp.write_all(NULL_RESPONSE),
            }
        }
        Some(Command::RPush { key, values }) => {
            let entry = store.entry(key).or_insert_with(|| Entry::List(Vec::new()));

            match entry {
                Entry::List(l) => {
                    l.extend(values);
                    _ = tcp.write_all(format!(":{}\r\n", l.len()).as_bytes())
                }
                Entry::Single { .. } => _ = tcp.write_all(NULL_RESPONSE),
            }
        }
        Some(Command::Echo(msg)) => {
            _ = tcp.write_all(format!("${}\r\n{}\r\n", msg.len(), msg).as_bytes());
        }
        Some(Command::Ping) => {
            _ = tcp.write_all(PONG_RESPONSE);
        }
        None => {
            _ = tcp.write_all(NULL_RESPONSE);
        }
    }
}
