#[path = "../socket.rs"]
mod socket;

use std::env;
use std::io::{self, BufReader, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        return;
    }

    let command = args.remove(0);
    let result = match command.as_str() {
        "set" => handle_set(args),
        "list" => handle_list(),
        "clients" => handle_clients(),
        "repl" => run_repl(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("Unknown command '{}'.", other);
            print_help();
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("prism: {}", err);
        std::process::exit(1);
    }
}

fn handle_set(args: Vec<String>) -> Result<(), String> {
    if args.len() < 2 {
        return Err("Usage: prism set <PID> <OFFSET>".to_string());
    }

    let pid: i32 = args[0]
        .parse()
        .map_err(|_| "PID must be an integer".to_string())?;
    let offset: u32 = args[1]
        .parse()
        .map_err(|_| "OFFSET must be a non-negative integer".to_string())?;

    let response = send_command(&format!("set {} {}", pid, offset))?;
    print!("{}", response);
    Ok(())
}

fn handle_list() -> Result<(), String> {
    let response = send_command("list")?;
    print!("{}", response);
    Ok(())
}

fn handle_clients() -> Result<(), String> {
    let response = send_command("clients")?;
    print!("{}", response);
    Ok(())
}

fn run_repl() -> Result<(), String> {
    println!("Prism control REPL (commands are routed via prismd). Type 'help' for commands.");
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        stdout
            .write_all(b"> ")
            .and_then(|_| stdout.flush())
            .map_err(|err| err.to_string())?;

        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match line {
            "help" => {
                print_help_repl();
                continue;
            }
            "exit" | "quit" => break,
            _ => {}
        }

        match send_command(line) {
            Ok(response) => print!("{}", response),
            Err(err) => eprintln!("prism: {}", err),
        }
    }

    Ok(())
}

fn send_command(command: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(socket::PRISM_SOCKET_PATH)
        .map_err(|err| format!("failed to connect to prismd: {}", err))?;

    stream
        .write_all(command.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|err| format!("failed to send command: {}", err))?;

    if let Err(err) = stream.shutdown(Shutdown::Write) {
        eprintln!("prism: warning: failed to half-close socket: {}", err);
    }

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader
        .read_to_string(&mut response)
        .map_err(|err| format!("failed to read response: {}", err))?;

    Ok(response)
}

fn print_help() {
    println!("Usage: prism <command> [args]\n");
    println!("Commands:");
    println!("  list                 Show driver properties via prismd");
    println!("  clients              Show active Prism clients via prismd");
    println!("  set <PID> <OFFSET>  Send routing update (relayed by prismd)");
    println!("  repl                 Start interactive shell (commands go to prismd)");
    println!("  help                 Show this help message");
}

fn print_help_repl() {
    println!("Commands:");
    println!("  set <PID> <OFFSET>  Send routing update (relayed by prismd)");
    println!("  list                 Show driver properties via prismd");
    println!("  clients              Show active Prism clients via prismd");
    println!("  exit                 Quit the shell");
}
