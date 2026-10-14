# rundb

Port em Rust do comando `wt db` (CLI Node.js) para executar scripts de banco
de dados SQL Server parametrizáveis por ambiente. Gera um único executável
estático (sem dependência de Node/npm), fácil de distribuir.

Os scripts `.js` continuam **exatamente no mesmo formato** do projeto
original (CommonJS: `module.exports = { database, mode, params, queries }`),
graças a um motor JavaScript (QuickJS via `rquickjs`) embutido no binário.

## Uso

```
rundb --file <caminho-do-script.js> [options]
rundb connection <add|list|remove|test>
```

Se `--file`/`-f` não for informado, lista os scripts disponíveis em
`./db-scripts` (ou no diretório definido em `--scripts-dir` / `DB_SCRIPTS_DIR`)
para escolha interativa.

### Options

| Flag             | Descrição                                                        |
|------------------|-------------------------------------------------------------------|
| `-f, --file`     | Caminho do arquivo de script (.js)                                |
| `-e, --env`      | Ambiente: dev \| stg \| prd (default: dev)                        |
| `-p, --param`    | Parâmetro no formato `chave=valor` (pode repetir)                 |
| `-j, --json`     | Exibe resultados em JSON puro                                     |
| `-d, --debug`    | Exibe SQL e resultado de todas as queries, não só a última        |
| `-h, --help`     | Mostra a mensagem de ajuda                                        |
| `--scripts-dir`  | Diretório onde os scripts `.js` estão (default: `./db-scripts`)    |

### Conexões armazenadas

As conexões podem ser cadastradas interativamente e ficam associadas à pasta dos
scripts:

```
rundb connection add
rundb connection list
rundb connection test users-dev
rundb connection remove users-dev
```

O cadastro inclui `name`, `adapter`, `environment`, `system`, `host`, `port`,
`database`, `username`, `password` e opções de criptografia. O adapter disponível
atualmente é `sqlserver`; o campo existe para permitir adapters futuros.

Os dados são gravados em `db-scripts/.rundb-connections.enc`, criptografados com
ChaCha20-Poly1305. A chave é mantida no cofre de credenciais do sistema. Quando
esse cofre não está disponível, o `rundb` solicita uma senha-mestra e usa Argon2
para derivar a chave. O arquivo nunca contém as senhas em texto puro e deve ser
adicionado ao `.gitignore`:

```
.rundb-connections.enc
```

### Exemplos

```
rundb --file db-scripts/find-user.js
rundb --file db-scripts/find-user.js --env stg
rundb --file db-scripts/find-user.js --param email=foo@bar.com --json
rundb --file db-scripts/find-user.js --debug
```

## Variáveis de ambiente esperadas

Para cada `database` declarado no script (`<SYSTEM>`), no ambiente
`<ENV>` escolhido via `--env`:

```
DB_<ENV>_<SYSTEM>_SVR   # host[:porta]
DB_<ENV>_<SYSTEM>_USR
DB_<ENV>_<SYSTEM>_PWD
DB_<ENV>_<SYSTEM>_DB    # opcional
```

Exemplo: `DB_DEV_USERS_SVR`, `DB_DEV_USERS_USR`, `DB_DEV_USERS_PWD`.

Durante a migração, uma conexão cadastrada tem prioridade. Se não houver uma
conexão para o par ambiente/sistema, as variáveis acima continuam sendo usadas.

## Formato dos scripts

Idêntico ao projeto original em Node — veja `db-scripts/example.js`.

```js
module.exports = {
  database: 'USERS',        // define o prefixo das envs DB_<ENV>_<SYSTEM>_*
  mode: 'sequential',        // 'sequential' (default) ou 'parallel'
  params: [
    { name: 'email', message: 'E-mail do usuário:' },
  ],
  queries: [
    {
      name: 'Buscar usuário pelo e-mail',
      sql: (p, prev) => `SELECT Id, Email FROM [User] WHERE Email = '${p.email}'`,
      // database: 'ORDERS',  // opcional: sobrescreve o database da query
      // debug: true,          // opcional: sempre imprime esta query
      // prompt: {             // opcional (modo sequential): prompt interativo
      //   message: 'Qual usuário?',
      //   valueField: 'Id',
      //   labelField: 'Email',
      //   paramName: 'userId',
      //   multiple: false,
      // },
    },
  ],
};
```

## Build

```
cargo build --release
```

O executável fica em `target/release/rundb` — um binário estático (linka
apenas libc/libm/libgcc do sistema), pronto para copiar para qualquer
máquina Linux compatível.

## Testando o carregamento dos scripts sem banco

```
cargo run --example load_check
```

Carrega todos os `.js` de `db-scripts/` e executa a função `sql()` de cada
query com parâmetros fictícios, sem conectar a nenhum banco — útil para
validar rapidamente que o motor JS interpreta os scripts corretamente.
