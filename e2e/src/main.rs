use std::{collections::BTreeMap, env, path::PathBuf, time::Duration};

use uze_conformance::{HarnessRunSpec, run};

fn main() {
    let mut arguments = env::args().skip(1);
    let Some(executable) = arguments.next() else {
        eprintln!("usage: uze-conformance <executable> [args...]");
        std::process::exit(64);
    };
    let result = run(&HarnessRunSpec {
        executable: PathBuf::from(executable),
        arguments: arguments.collect(),
        environment: BTreeMap::new(),
        home: PathBuf::from("/work/home"),
        uze_home: PathBuf::from("/work/uze-home"),
        working_directory: PathBuf::from("/work/project"),
        stdin: None,
        timeout: Duration::from_secs(30),
    });
    match result {
        Ok(run) => {
            print!("{}", String::from_utf8_lossy(&run.stdout));
            eprint!("{}", String::from_utf8_lossy(&run.stderr));
            std::process::exit(run.exit_code.unwrap_or(1));
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
