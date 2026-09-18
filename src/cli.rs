use clap::Parser;

/// Executa scripts de banco de dados (SQL Server) parametrizáveis por ambiente.
#[derive(Parser, Debug)]
#[command(name = "rundb", disable_help_flag = true, disable_version_flag = true)]
pub struct Cli {
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
    #[arg(long = "scripts-dir")]
    pub scripts_dir: Option<String>,
}
