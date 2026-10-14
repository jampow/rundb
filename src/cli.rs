use clap::{Args, Parser, Subcommand};

/// Executa scripts de banco de dados (SQL Server) parametrizáveis por ambiente.
#[derive(Parser, Debug)]
#[command(name = "rundb", disable_help_flag = true, disable_version_flag = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Gerencia conexões armazenadas junto aos scripts
    Connection(ConnectionArgs),
}

#[derive(Args, Debug)]
pub struct ConnectionArgs {
    #[command(subcommand)]
    pub action: ConnectionAction,
}

#[derive(Subcommand, Debug)]
pub enum ConnectionAction {
    /// Cadastra ou substitui uma conexão
    Add,
    /// Lista conexões cadastradas
    List,
    /// Remove uma conexão
    Remove {
        /// Nome da conexão
        name: String,
    },
    /// Testa uma conexão
    Test {
        /// Nome da conexão
        name: String,
    },
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Caminho do arquivo de script (.js)
    #[arg(short = 'f', long = "file")]
    pub file: Option<String>,

    /// Ambiente: dev | stg | prd (default: dev)
    #[arg(short = 'e', long = "env", default_value = "dev")]
    pub env: String,

    /// Parâmetro no formato chave=valor (pode repetir)
    #[arg(short = 'p', long = "param")]
    pub param: Vec<String>,

    /// Exibe resultados em JSON puro
    #[arg(short = 'j', long = "json")]
    pub json: bool,

    /// Exibe SQL e resultado de todas as queries, não só a última
    #[arg(short = 'd', long = "debug")]
    pub debug: bool,

    /// Mostra esta mensagem
    #[arg(short = 'h', long = "help")]
    pub help: bool,

    /// Diretório onde os scripts (.js) estão localizados (default: ./db-scripts)
    #[arg(long = "scripts-dir", global = true)]
    pub scripts_dir: Option<String>,
}

impl std::ops::Deref for Cli {
    type Target = RunArgs;

    fn deref(&self) -> &Self::Target {
        &self.run
    }
}
