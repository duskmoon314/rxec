use std::{collections::VecDeque, path::PathBuf, process::Stdio, sync::atomic::AtomicUsize};

use clap::Parser;

use config::{gen_template, load_config, Cli, Conf};
use tokio::{io::AsyncReadExt, runtime::Runtime, task::JoinSet};

mod config;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(config::Commands::Template { path }) = cli.cmd.clone() {
        gen_template(path)?;
        return Ok(());
    }

    let conf = load_config(cli);

    // Configure tokio runtime
    let mut rt = tokio::runtime::Builder::new_multi_thread();

    // Configure threads
    if let Some(threads) = conf.threads {
        rt.worker_threads(threads as usize);
    } else if let None = conf.parallel {
        rt.worker_threads(1);
    }
    rt.thread_name_fn(|| {
        static THREAD_ID: AtomicUsize = AtomicUsize::new(0);
        let id = THREAD_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("rxec-worker-{id}")
    });

    // Enable IO, time...
    rt.enable_all();

    // Build runtime
    let rt = rt.build()?;

    // Run
    run(conf, rt)?;

    Ok(())
}

#[derive(Clone, Debug)]
struct Task {
    pub cmd: String,
    pub args: Vec<String>,
    pub number: u32,
}

#[derive(Debug)]
struct Tasks(pub VecDeque<Task>);

impl Tasks {
    pub fn pop(&mut self) -> Option<Task> {
        if let Some(task) = self.0.front_mut() {
            if task.number > 1 {
                task.number -= 1;
                Some(task.clone())
            } else {
                let mut task = self.0.pop_front().unwrap();
                task.number -= 1;
                Some(task)
            }
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn run(conf: Conf, rt: Runtime) -> anyhow::Result<()> {
    let tasks: VecDeque<Task> = conf
        .args
        .iter()
        .map(|arg| {
            let cmd = conf.cmd[0].clone();
            let mut args = conf.cmd[1..].to_vec();
            if arg != "" {
                args.push(arg.clone())
            }

            Task {
                cmd,
                args,
                number: conf.number,
            }
        })
        .collect();
    let mut tasks = Tasks(tasks);

    if tasks.is_empty() {
        return Ok(());
    }

    let interval = if conf.parallel.is_some() {
        None
    } else {
        Some(conf.interval)
    };
    let output_path = conf.output.unwrap_or(conf.cmd[0].clone())
        + chrono::Local::now()
            .format("-%Y%m%d%H%M%S")
            .to_string()
            .as_str();
    std::fs::create_dir(&output_path)?;

    rt.block_on(async {
        let mut set = JoinSet::new();

        // Push conf.parallel tasks to set
        match conf.parallel {
            None => {
                set.spawn({
                    // Push the first task to set
                    let task = tasks.pop().unwrap();
                    exec(task, conf.cwd.clone(), conf.timeout, interval)
                });
            }
            Some(0) => {
                // Push all tasks to set
                while let Some(task) = tasks.pop() {
                    set.spawn(exec(task, conf.cwd.clone(), conf.timeout, interval));
                }
            }
            Some(n) => {
                // Push n tasks to set
                let mut pushed = 0;
                while let Some(task) = tasks.pop() {
                    set.spawn(exec(task, conf.cwd.clone(), conf.timeout, interval));
                    pushed += 1;
                    if pushed >= n {
                        break;
                    }
                }
            }
        }

        // Push tasks if set is not full
        while let Some(res) = set.join_next().await {
            if !tasks.is_empty() {
                set.spawn(exec(
                    tasks.pop().unwrap(),
                    conf.cwd.clone(),
                    conf.timeout,
                    interval,
                ));
            }

            // Save output if status is not ok
            match res {
                Ok(Ok((arg, num, output))) => {
                    let out_log = format!("{output_path}/{arg}-{num}.log");

                    tokio::fs::write(out_log, output.stdout)
                        .await
                        .expect("Failed to write output");
                }
                _ => {
                    // TODO: Log
                }
            }
        }
    });

    Ok(())
}

async fn exec(
    task: Task,
    cwd: PathBuf,
    timeout: Option<u32>,
    interval: Option<u32>,
) -> anyhow::Result<(String, u32, std::process::Output)> {
    println!("timeout = {:?}", timeout);

    let mut command = tokio::process::Command::new(&task.cmd);
    command.args(&task.args);
    command.current_dir(cwd);
    command.stdout(Stdio::piped());
    let mut child = command.spawn()?;

    let sleep = tokio::time::sleep(tokio::time::Duration::from_secs(timeout.unwrap_or(0) as u64));
    tokio::pin!(sleep);

    let res = tokio::select! {
        _ = &mut sleep, if timeout.is_some() => {
            // Kill the process
            child.kill().await?;

            Err(anyhow::anyhow!("Timeout"))
        }

        res = child.wait() => {
            if let Ok(status) = res {
                let mut outbuf = Vec::new();
                if let Some(mut stdout) = child.stdout.take() {
                    stdout.read_to_end(&mut outbuf).await?;
                }

                let mut errbuf = Vec::new();
                if let Some(mut stderr) = child.stderr.take() {
                    stderr.read_to_end(&mut errbuf).await?;
                }

                let output = std::process::Output {
                    status,
                    stdout: outbuf,
                    stderr: errbuf,
                };

                Ok(output)
            } else {
                Err(anyhow::anyhow!("Failed to wait child process"))
            }
        }
    };

    if let Some(interval) = interval {
        tokio::time::sleep(tokio::time::Duration::from_secs(interval as u64)).await;
    }

    res.map(|output| {
        (
            task.args.last().unwrap_or(&"".to_string()).clone(),
            task.number,
            output,
        )
    })
}
