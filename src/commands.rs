use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    net::TcpStream,
    time::{Duration, Instant},
};

const ECHO_PREFIX: &[u8; 14] = b"*2\r\n$4\r\nECHO\r\n";
const SET_PREFIX: &[u8; 13] = b"*3\r\n$3\r\nSET\r\n";
const SET_EXP_PREFIX: &[u8; 13] = b"*5\r\n$3\r\nSET\r\n";
const GET_PREFIX: &[u8; 13] = b"*2\r\n$3\r\nGET\r\n";
const PING_PREFIX: &[u8; 14] = b"*1\r\n$4\r\nPING\r\n";
const LLEN_PREFIX: &[u8; 14] = b"*2\r\n$4\r\nLLEN\r\n";

const PONG_RESPONSE: &[u8; 7] = b"+PONG\r\n";
const NULL_RESPONSE: &[u8; 5] = b"$-1\r\n";
const OK_RESPONSE: &[u8; 5] = b"+OK\r\n";
const ZERO_ARRAY: &[u8; 4] = b"*0\r\n";

pub enum Entry {
    Single {
        value: String,
        expiry: Option<Instant>,
    },
    List(VecDeque<String>),
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
        list_key: String,
        values: Vec<String>,
    },
    LRange {
        list_key: String,
        low: i8,
        high: i8,
    },
    LPush {
        list_key: String,
        values: Vec<String>,
    },
    LLen(String),
    LPop { key: String, count: usize },
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

fn parse_rpush(buf: &[u8]) -> Option<(String, Vec<String>)> {
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

    Some((l.remove(0), l))
}

fn parse_lpush(buf: &[u8]) -> Option<(String, Vec<String>)> {
    let mut l: Vec<String> = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .filter(|x| !x.is_empty())
        .filter(|x| !x.starts_with("$"))
        .filter(|x| !x.starts_with("*"))
        .filter(|x| *x != "LPUSH")
        .map(|x| x.to_owned())
        .collect();

    if l.is_empty() {
        return None;
    }

    Some((l.remove(0), l))
}

fn parse_lrange(buf: &[u8]) -> Option<(String, i8, i8)> {
    let mut l: Vec<String> = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .filter(|x| !x.is_empty())
        .filter(|x| !x.starts_with("$"))
        .filter(|x| !x.starts_with("*"))
        .filter(|x| *x != "LRANGE")
        .map(|x| x.to_owned())
        .collect();

    if l.is_empty() {
        return None;
    }

    Some((
        l.remove(0),
        l.get(0)?.parse::<i8>().ok()?,
        l.get(1)?.parse::<i8>().ok()?,
    ))
}

fn parse_lpop(buf: &[u8]) -> Option<(String, usize)> {
    let parts: Vec<String> = std::str::from_utf8(buf)
        .ok()?
        .split("\r\n")
        .filter(|x| !x.is_empty())
        .filter(|x| !x.starts_with("$"))
        .filter(|x| !x.starts_with("*"))
        .map(|x| x.to_owned())
        .collect();

    let key = parts.get(1)?.clone();
    let count = parts.get(2).and_then(|c| c.parse::<usize>().ok()).unwrap_or(1);
    Some((key, count))
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
    } else if buf.starts_with(LLEN_PREFIX) {
        parse_v(&buf[LLEN_PREFIX.len()..]).map(Command::LLen)
    } else if buf.windows(b"LPOP".len()).any(|w| w == b"LPOP") {
        parse_lpop(buf).map(|(key, count)| Command::LPop { key, count })
    } else if buf.windows(b"RPUSH".len()).any(|w| w == b"RPUSH") {
        parse_rpush(buf).map(|(k, vl)| Command::RPush {
            list_key: k,
            values: vl,
        })
    } else if buf.windows(b"LPUSH".len()).any(|w| w == b"LPUSH") {
        parse_lpush(buf).map(|(k, vl)| Command::LPush {
            list_key: k,
            values: vl,
        })
    } else if buf.windows(b"LRANGE".len()).any(|w| w == b"LRANGE") {
        parse_lrange(buf).map(|(list_key, low, high)| Command::LRange {
            list_key,
            low,
            high,
        })
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
                Some(Entry::List(_)) => _ = tcp.write_all(NULL_RESPONSE),
                None => _ = tcp.write_all(NULL_RESPONSE),
            }
        }
        Some(Command::LPush {
            list_key: key,
            values,
        }) => {
            let entry = store.entry(key).or_insert_with(|| Entry::List(VecDeque::new()));

            match entry {
                Entry::List(l) => {
                    for v in values {
                        l.push_front(v);
                    }
                    _ = tcp.write_all(format!(":{}\r\n", l.len()).as_bytes())
                }
                Entry::Single { .. } => _ = tcp.write_all(NULL_RESPONSE),
            }
        }
        Some(Command::RPush {
            list_key: key,
            values,
        }) => {
            let entry = store.entry(key).or_insert_with(|| Entry::List(VecDeque::new()));

            match entry {
                Entry::List(l) => {
                    for v in values {
                        l.push_back(v);
                    }
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
        Some(Command::LRange {
            list_key,
            low,
            high,
        }) => {
            let entry = store.get(&list_key);

            match entry {
                Some(Entry::List(l)) => {
                    let len = l.len() as isize;
                    let mut low = if low < 0 { len + low as isize } else { low as isize };
                    let mut high = if high < 0 { len + high as isize } else { high as isize };

                    if low < 0 {
                        low = 0
                    };
                    if high < 0 {
                        high = 0
                    };

                    if low >= len || low > high {
                        _ = tcp.write_all(ZERO_ARRAY);
                        return;
                    }
                    if high >= len {
                        high = len - 1;
                    }
                    let mut response = format!("*{}\r\n", high - low + 1);
                    for i in low..=high {
                        if let Some(item) = l.get(i as usize) {
                            response.push_str(&format!("${}\r\n{}\r\n", item.len(), item));
                        }
                    }
                    _ = tcp.write_all(response.as_bytes());
                }
                Some(Entry::Single { .. }) => _ = tcp.write_all(ZERO_ARRAY),
                None => _ = tcp.write_all(ZERO_ARRAY),
            }
        }
        Some(Command::LLen(key)) => {
            let entry = store.entry(key).or_insert_with(|| Entry::List(VecDeque::new()));

            match entry {
                Entry::List(l) => _ = tcp.write_all(format!(":{}\r\n", l.len()).as_bytes()),
                Entry::Single { .. } => _ = tcp.write_all(NULL_RESPONSE),
            }
        }
        Some(Command::LPop { key, count }) => {
            match store.get_mut(&key) {
                Some(Entry::List(l)) => {
                    let pop_count = count.min(l.len());
                    if pop_count == 0 {
                        _ = tcp.write_all(NULL_RESPONSE);
                        return;
                    }

                    let mut values: Vec<String> = Vec::with_capacity(pop_count);
                    for _ in 0..pop_count {
                        if let Some(v) = l.pop_front() {
                            values.push(v);
                        }
                    }

                    if values.len() == 1 && count == 1 {
                        _ = tcp.write_all(format!("${}\r\n{}\r\n", values[0].len(), values[0]).as_bytes());
                    } else {
                        let mut response = format!("*{}\r\n", values.len());
                        for v in &values {
                            response.push_str(&format!("${}\r\n{}\r\n", v.len(), v));
                        }
                        _ = tcp.write_all(response.as_bytes());
                    }

                    if l.is_empty() {
                        store.remove(&key);
                    }
                }
                Some(Entry::Single { .. }) => _ = tcp.write_all(NULL_RESPONSE),
                None => _ = tcp.write_all(NULL_RESPONSE),
            }
        }
        None => {
            _ = tcp.write_all(NULL_RESPONSE);
        }
    }
}
