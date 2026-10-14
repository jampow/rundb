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
    Add(ConnectionAddArgs),
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
pub struct ConnectionAddArgs {
    /// Nome identificador da conexão
    #[arg(long)]
    pub name: Option<String>,

    /// Adapter da conexão (default: sqlserver)
    #[arg(long, default_value = "sqlserver")]
    pub adapter: String,

    /// Ambiente, por exemplo dev, stg ou prd
    #[arg(long, alias = "environment")]
    pub env: Option<String>,

    /// Sistema usado no campo database do script
    #[arg(long)]
    pub system: Option<String>,

    /// Host do banco
    #[arg(long)]
    pub host: Option<String>,

    /// Porta do banco (default: 1433)
    #[arg(long, default_value_t = 1433)]
    pub port: u16,

    /// Database inicial (opcional)
    #[arg(long)]
    pub database: Option<String>,

    /// Usuário do banco
    #[arg(long)]
    pub username: Option<String>,

    /// Não exige criptografia TLS na conexão
    #[arg(long)]
    pub no_encrypt: bool,

    /// Não confia automaticamente no certificado do servidor
    #[arg(long)]
    pub no_trust_certificate: bool,
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
