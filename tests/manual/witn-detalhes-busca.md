# WITN — busca avançada, menu de contexto e Detalhes
produto: WITN (Foundry32)
build: C:\Users\Pichau\sandbox\mcp-hangar\target\release\witn-gui.exe (branch feat/witn-details-search)

## SC-01 · Preparar o caso: uma pasta travada por um node órfão
tags: preparo
pré: PowerShell aberto em C:\Users\Pichau\sandbox

Reproduz o incidente de 22/10: um node.exe órfão, com uma variável "secreta", ouvindo a porta 4599, parado dentro de uma pasta.

1. Rodar `mkdir _witn-test; Set-Content _witn-test\server.js "require('http').createServer().listen(4599)"`
   → a pasta _witn-test é criada com o arquivo server.js
2. Rodar `pwsh -NoProfile -c '$env:WITN_TEST_TOKEN=''segredo123''; Start-Process node -ArgumentList server.js -WorkingDirectory C:\Users\Pichau\sandbox\_witn-test -WindowStyle Hidden'`
   → o comando volta ao prompt sem abrir janela (a tentativa de apagar a pasta fica para o SC-07)

## SC-02 · A busca acha a pasta e diz onde achou
tags: busca

1. Abrir o witn-gui.exe da build acima → a lista de node aparece, com uma caixa de busca no topo e o texto "Buscar em tudo: pasta, diretório de trabalho…"
2. Digitar `_witn-test` na busca
   → sobra uma linha ("server", porta 4599); surge a coluna "Encontrado em" com "Local do app, Diretório de trabalho"; a barra de baixo diz "1 de N processos node · busca: _witn-test"
3. Trocar o texto por `*witn-test*` → o mesmo resultado
4. Trocar o texto por `4599` → a mesma linha, com "Encontrado em: Porta"
5. Apertar Esc com o cursor na busca → a busca limpa, a coluna "Encontrado em" some e a lista volta em árvore

## SC-03 · Menu do clique direito e a janela Detalhes
tags: menu, detalhes

1. Buscar `_witn-test` e clicar com o botão direito na linha
   → menu com "Detalhes" em negrito, "Abrir local do app", "Abrir diretório de trabalho", "Copiar ▸" e "Encerrar árvore de processos…"
2. Clicar em "Detalhes"
   → abre "Detalhes — server (PID n)" com as abas Geral, Origem, Desempenho, Rede, Ambiente; as listas preenchem as abas sem sobrar nem cortar
3. Aba Geral → "Diretório de trabalho (cwd)" mostra C:\Users\Pichau\sandbox\_witn-test\ e "Versão do Node" mostra 22.22.0
4. Aba Origem
   → "Processo pai" = "<encerrado> (PID …)", "Órfão" = "Sim", "Iniciado por" = onde você rodou o comando (Windows Terminal; ou Claude Code, se rodou pelo `!`), com "Evidência: variáveis de ambiente: …"
5. Aba Desempenho → CPU, Memória, Handles, Threads e "Prioridade: Normal" preenchidos
6. Aba Rede → duas linhas LISTENING: TCP 0.0.0.0:4599 e TCPv6 [::]:4599

## SC-04 · Valores sensíveis e cópia
tags: detalhes, segredos

1. Aba Ambiente → a linha WITN_TEST_TOKEN mostra "••••••••", não "segredo123"
2. Marcar "Mostrar valores sensíveis" → WITN_TEST_TOKEN passa a mostrar "segredo123"
3. Desmarcar a caixa, clicar com o botão direito na linha PATH e escolher "Copiar valor", depois colar no Bloco de notas
   → aparece o conteúdo do PATH; a barra da janela diz "Copiado para a área de transferência."
4. Clicar em "Copiar tudo" e colar no Bloco de notas
   → um relatório com GERAL, ORIGEM, DESEMPENHO, REDE e AMBIENTE; WITN_TEST_TOKEN aparece como "••••••••"

## SC-05 · Abrir local e diretório de trabalho
tags: menu

1. Na janela principal, selecionar a linha do teste e clicar no botão "Abrir local"
   → o Explorer abre a pasta _witn-test com o server.js já selecionado
2. Clique direito → "Abrir diretório de trabalho" → o Explorer abre a pasta _witn-test
3. Clique direito → Copiar ▸ PID e colar no Bloco de notas → aparece o PID da linha

## REG-01 · Enter numa linha abre Detalhes
tags: regressão, teclado

Corrigido na revisão: o Enter era engolido pelo loop de mensagens e não abria nada.

1. Fechar as janelas Detalhes, clicar numa linha da lista e apertar Enter → a janela Detalhes daquele processo abre
2. Voltar à janela principal e dar duplo clique na mesma linha → a janela que já estava aberta vem para a frente; nenhuma segunda janela abre

## REG-02 · Limpar a busca volta à árvore mesmo ordenando por "Encontrado em"
tags: regressão, busca

Corrigido na revisão: a lista ficava achatada e sem cabeçalho para voltar.

1. Digitar `node` na busca e clicar no cabeçalho "Encontrado em" → a barra de baixo mostra "ordem: Encontrado em ↑"
2. Apertar Esc na busca → a coluna some, a lista volta indentada em árvore e a barra não fala mais em "ordem"

## SC-06 · Atalhos de teclado
tags: teclado

1. Com o foco na lista, apertar Ctrl+F → o cursor vai para a busca com o texto selecionado
2. Com o cursor na busca, apertar Delete → apaga texto da busca; nenhum processo é encerrado

## SC-07 · Fechar o caso: a pasta trava, o WITN encerra, a pasta apaga
tags: encerrar
pré: janela Detalhes do "server" aberta

1. No PowerShell, rodar `Remove-Item .\_witn-test -Recurse -Force`
   → erro "being used by another process", o mesmo de hoje (o server.js some; a pasta fica)
2. Na janela principal, selecionar a linha do teste e apertar Delete
   → pede confirmação: "Encerrar server e toda a sua árvore de processos?"
3. Confirmar → a linha some da lista na próxima atualização
4. Na janela Detalhes, clicar em "Atualizar" → aparece "O processo foi encerrado; mostrando a última leitura."
5. No PowerShell, rodar de novo `Remove-Item .\_witn-test -Recurse -Force` → apaga sem erro
