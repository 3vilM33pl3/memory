use std::collections::HashMap;
use std::process::Command;

#[derive(Debug)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    pub rss_kb: u64,
    pub cpu_pct: f64,
    pub command: String,
}

pub fn get_process_info() -> HashMap<u32, ProcInfo> {
    mem_platform::process_snapshots()
        .into_iter()
        .map(|process| {
            (
                process.pid,
                ProcInfo {
                    pid: process.pid,
                    ppid: process.parent_pid,
                    rss_kb: process.memory_kb,
                    cpu_pct: process.cpu_percent,
                    command: process.command,
                },
            )
        })
        .collect()
}

pub fn get_children_map(procs: &HashMap<u32, ProcInfo>) -> HashMap<u32, Vec<u32>> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for proc in procs.values() {
        children.entry(proc.ppid).or_default().push(proc.pid);
    }
    children
}

pub fn has_active_descendant(
    pid: u32,
    children_map: &HashMap<u32, Vec<u32>>,
    process_info: &HashMap<u32, ProcInfo>,
    cpu_threshold: f64,
) -> bool {
    let mut stack = vec![pid];
    while let Some(p) = stack.pop() {
        if let Some(kids) = children_map.get(&p) {
            for &kid in kids {
                if process_info
                    .get(&kid)
                    .is_some_and(|p| p.cpu_pct > cpu_threshold)
                {
                    return true;
                }
                stack.push(kid);
            }
        }
    }
    false
}

pub fn get_listening_ports() -> HashMap<u32, Vec<u16>> {
    #[cfg(target_os = "windows")]
    {
        let output = std::process::Command::new("netstat.exe")
            .args(["-ano", "-p", "tcp"])
            .output()
            .ok();
        output
            .filter(|output| output.status.success())
            .map(|output| parse_windows_netstat(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or_default()
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut map: HashMap<u32, Vec<u16>> = HashMap::new();
        let output = Command::new("lsof")
            .args(["-i", "-P", "-n", "-sTCP:LISTEN"])
            .output()
            .ok();

        if let Some(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                let is_tcp_listen =
                    parts.len() >= 9 && parts[7] == "TCP" && line.contains("(LISTEN)");
                if is_tcp_listen
                    && let Ok(pid) = parts[1].parse::<u32>()
                    && let Some(addr) = parts.get(8)
                    && let Some(port_str) = addr.rsplit(':').next()
                    && let Ok(port) = port_str.parse::<u16>()
                {
                    map.entry(pid).or_default().push(port);
                }
            }
        }
        map
    }
}

#[cfg(target_os = "windows")]
pub fn parse_windows_netstat(output: &str) -> HashMap<u32, Vec<u16>> {
    let mut map: HashMap<u32, Vec<u16>> = HashMap::new();
    for line in output.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5
            || !parts[0].eq_ignore_ascii_case("TCP")
            || !parts[3].eq_ignore_ascii_case("LISTENING")
        {
            continue;
        }
        let Some(port_text) = parts[1].rsplit(':').next() else {
            continue;
        };
        if let (Ok(port), Ok(pid)) = (port_text.parse::<u16>(), parts[4].parse::<u32>()) {
            map.entry(pid).or_default().push(port);
        }
    }
    map
}

/// Check if a command string has a given binary name in executable position.
/// Checks the first two argv tokens only (covers direct invocation and
/// interpreter-wrapped scripts like `node /path/to/codex ...`).
pub fn cmd_has_binary(cmd: &str, name: &str) -> bool {
    command_tokens(cmd, 2)
        .into_iter()
        .any(|token| token_is_binary(token, name))
}

pub fn cmd_starts_with_binary(cmd: &str, name: &str) -> bool {
    command_tokens(cmd, 1)
        .first()
        .is_some_and(|token| token_is_binary(token, name))
}

pub fn path_tail(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("?")
}

fn token_is_binary(token: &str, name: &str) -> bool {
    let expected = name.to_ascii_lowercase();
    let base = token.rsplit(['/', '\\']).next().unwrap_or(token);
    let lower = base.to_ascii_lowercase();
    lower == expected
        || [".exe", ".cmd", ".bat", ".ps1", ".js"]
            .iter()
            .any(|suffix| {
                lower
                    .strip_suffix(suffix)
                    .is_some_and(|stem| stem == expected)
            })
}

fn command_tokens(command: &str, limit: usize) -> Vec<&str> {
    let bytes = command.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() && tokens.len() < limit {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        let quoted = bytes[index] == b'"';
        if quoted {
            index += 1;
        }
        let start = index;
        while index < bytes.len()
            && if quoted {
                bytes[index] != b'"'
            } else {
                !bytes[index].is_ascii_whitespace()
            }
        {
            index += 1;
        }
        tokens.push(&command[start..index]);
        if quoted && index < bytes.len() {
            index += 1;
        }
    }
    tokens
}

pub fn collect_git_stats(cwd: &str) -> (u32, u32) {
    let output = Command::new("git")
        .args(["-C", cwd, "status", "--porcelain"])
        .output()
        .ok();

    let mut added = 0u32;
    let mut modified = 0u32;

    if let Some(output) = output
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.len() < 2 {
                continue;
            }
            let status_code = &line[..2];
            if status_code.contains('?') || status_code.contains('A') {
                added += 1;
            } else if status_code.contains('M') {
                modified += 1;
            }
        }
    }

    (added, modified)
}

#[cfg(test)]
mod tests {
    use super::{cmd_has_binary, cmd_starts_with_binary, path_tail};

    #[test]
    fn command_detection_accepts_windows_paths_and_exe_suffixes() {
        assert!(cmd_has_binary(
            r#""C:\Users\Test User\AppData\Roaming\npm\codex.exe" --resume"#,
            "codex"
        ));
        assert!(cmd_has_binary(
            r#"node.exe C:\Users\tester\AppData\Roaming\npm\claude.exe"#,
            "claude"
        ));
        assert!(cmd_has_binary(
            r#"node.exe C:\Users\tester\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js"#,
            "codex"
        ));
        assert!(!cmd_has_binary(r#"C:\Tools\codec.exe"#, "codex"));
        assert!(cmd_starts_with_binary("node.exe codex.js", "node"));
        assert_eq!(path_tail(r"C:\Users\tester\memory"), "memory");
        assert_eq!(path_tail("/home/tester/memory/"), "memory");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_netstat_parser_finds_ipv4_and_ipv6_listeners() {
        let parsed = super::parse_windows_netstat(
            "  Proto  Local Address          Foreign Address        State           PID\r\n\
             TCP    127.0.0.1:4040         0.0.0.0:0              LISTENING       1234\r\n\
             TCP    [::1]:4041             [::]:0                 LISTENING       1234\r\n\
             TCP    127.0.0.1:5432         127.0.0.1:61234        ESTABLISHED     9999\r\n",
        );
        assert_eq!(parsed.get(&1234), Some(&vec![4040, 4041]));
        assert!(!parsed.contains_key(&9999));
    }
}
