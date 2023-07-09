use std::path::PathBuf;

use clap::{value_parser, Args, Parser, Subcommand};
use confique::Config;

#[derive(Args, Config, Debug)]
pub struct Conf {
    /// The main part of command to run
    ///
    /// The first element is the command to run, and the rest are arguments that
    /// will be shared by all commands
    #[arg(trailing_var_arg = true)]
    pub cmd: Vec<String>,

    /// The arguments to form the full command
    #[arg(short, default_value = "", value_delimiter = ',')]
    pub args: Vec<String>,

    /// The working directory to run the command
    ///
    /// If not specified, the current working directory is used
    #[arg(short, default_value = ".")]
    pub cwd: PathBuf,

    /// The timeout in seconds
    ///
    /// If not specified, the command will not be timed out
    #[arg(short)]
    pub timeout: Option<u32>,

    /// The interval between two commands in seconds
    ///
    /// The default value is 0, which means commands are executed in sequence
    /// without any interval
    ///
    /// This will take effect only when [parallel](#structfield.parallel) is NOT
    /// set
    #[arg(short, default_value = "0")]
    #[config(default = 0)]
    pub interval: u32,

    /// Whether to run commands in parallel
    ///
    /// If None, commands are executed in sequence
    ///
    /// If Some(<number>), <number> commands are executed in parallel. If the
    /// <number> is 0, all commands are executed in parallel.
    #[arg(short)]
    pub parallel: Option<u32>,

    /// How many time to execute one command
    #[arg(short, default_value = "1", value_parser = value_parser!(u32).range(1..))]
    #[config(default = 1)]
    pub number: u32,

    /// Log output prefix
    ///
    /// If not specified, the command name is used
    #[arg(short)]
    pub output: Option<String>,

    /// The number of threads to use
    ///
    /// If not specified, the number of threads is equal to the number of CPUs
    #[arg(long)]
    pub threads: Option<u32>,
}

type PartialConf = <Conf as Config>::Partial;

#[derive(Clone, Debug, Subcommand)]
pub enum Commands {
    /// Generate config template
    Template {
        /// The path to save the template of config
        #[arg(default_value = "rxec.toml")]
        path: PathBuf,
    },
}

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Commands>,

    #[command(flatten)]
    conf: Conf,

    /// The config file to use
    #[arg(long, default_value = "rxec.toml")]
    config: PathBuf,
}

pub fn load_config(cli: Cli) -> Conf {
    // Convert the cli args to Conf::Partial
    let cli_conf: PartialConf = PartialConf {
        cmd: Some(cli.conf.cmd),
        args: Some(cli.conf.args),
        cwd: Some(cli.conf.cwd),
        timeout: cli.conf.timeout,
        interval: Some(cli.conf.interval),
        parallel: cli.conf.parallel,
        number: Some(cli.conf.number),
        output: cli.conf.output,
        threads: cli.conf.threads,
    };

    let conf = Conf::builder()
        .preloaded(cli_conf)
        .file(cli.config)
        .load()
        .expect("Failed to load config");

    conf
}

pub fn gen_template(path: PathBuf) -> anyhow::Result<()> {
    // Check the extension of the template file.
    let ext = path.extension().expect("template file has no extension");
    match ext.to_str() {
        Some("toml") => {
            let temp = confique::toml::template::<Conf>(confique::toml::FormatOptions::default());
            std::fs::write(path, temp)?;
            Ok(())
        }
        Some("yaml") | Some("yml") => {
            let temp = confique::yaml::template::<Conf>(confique::yaml::FormatOptions::default());
            std::fs::write(path, temp)?;
            Ok(())
        }
        Some("json5") => {
            let temp = confique::json5::template::<Conf>(confique::json5::FormatOptions::default());
            std::fs::write(path, temp)?;
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "template file extension must be one of toml, yaml, json5"
        )),
    }
}
