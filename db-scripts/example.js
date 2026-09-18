/**
 * Exemplo de script para o comando `wt db`.
 *
 * Uso:
 *   wt db --file db-scripts/example.js
 *   wt db --file db-scripts/example.js --env stg
 *   wt db --file db-scripts/example.js --param email=foo@bar.com --json
 *   wt db --file db-scripts/example.js --debug
 *
 * Variáveis de ambiente esperadas (para o "database" abaixo):
 *   DB_DEV_USERS_SVR / DB_DEV_USERS_USR / DB_DEV_USERS_PWD / DB_DEV_USERS_DB (opcional)
 *   DB_STG_USERS_SVR / ...
 *   DB_PRD_USERS_SVR / ...
 */

module.exports = {
  // Identificador do sistema/banco — compõe o nome das envs:
  // DB_<ENV>_<DATABASE>_*
  database: 'USERS',

  // 'sequential' (default) ou 'parallel'.
  // Em 'sequential', cada query recebe o recordset da query anterior em `prev`.
  mode: 'sequential',

  // Lista de parâmetros que o script precisa.
  // Se o usuário não passar via --param chave=valor, será solicitado via prompt.
  params: [
    { name: 'email', message: 'E-mail do usuário:' },
    // { name: 'dias', message: 'selecione a quantidade de dias', choices: ['oneday', 'twodays'] },
  ],

  queries: [
    {
      name: 'Buscar usuário pelo e-mail',
      sql: (p) => `SELECT Id, Email, Idb2c FROM [User] WHERE Email = '${p.email}'`,

      // Opcional: após esta query, exibe um prompt interativo com os registros
      // retornados como opções. O valor escolhido é injetado em `p` para as
      // queries seguintes.
      //
      // prompt: {
      //   message:    'Qual usuário?',   // texto exibido no prompt
      //   valueField: 'Id',              // campo do recordset usado como valor
      //   labelField: 'Email',           // campo exibido como label
      //   paramName:  'userId',          // chave injetada em params (p.userId)
      //   multiple:   false,             // false → select (padrão); true → checkbox (array)
      // },
    },

    // Exemplo: selecionar múltiplos registros para a próxima query
    // {
    //   name: 'Listar roles disponíveis',
    //   sql: () => `SELECT Id, Name FROM [Role] ORDER BY Name`,
    //   prompt: {
    //     message:    'Quais roles atribuir?',
    //     valueField: 'Id',
    //     labelField: 'Name',
    //     paramName:  'roleIds',
    //     multiple:   true,              // retorna array em p.roleIds
    //   },
    // },
    // {
    //   name: 'Atribuir roles ao usuário',
    //   sql: (p) => p.roleIds
    //     .map(roleId => `INSERT INTO [UserRole] (UserId, RoleId) VALUES ('${p.userId}', '${roleId}')`)
    //     .join('\n'),
    // },

    // Exemplo: query em um banco diferente (abre pool separada para DB_<ENV>_ORDERS_*)
    // {
    //   name: 'Buscar pedidos do usuário',
    //   database: 'ORDERS',   // sobrescreve o database padrão do script
    //   sql: (p) => `SELECT Id, Total FROM [Order] WHERE UserId = '${p.userId}'`,
    // },

    // Exemplo usando o resultado da query anterior via `prev`:
    // {
    //   name: 'Buscar permissões',
    //   sql: (p, prev) => {
    //     const userId = prev[0]?.Id;
    //     return `SELECT * FROM [Permission] WHERE UserId = '${userId}'`;
    //   },
    // },
  ],
};
