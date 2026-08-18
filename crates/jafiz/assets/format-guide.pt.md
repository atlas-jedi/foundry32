# Formato de suíte JAFIZ

Um arquivo `.md` por suíte. Markdown comum — renderiza no GitHub e em
qualquer editor. O JAFIZ nunca reescreve este arquivo: o progresso da
execução fica em `.runs/<nome>.json`, ao lado.

## Regras

1. O primeiro `# ` é o título da suíte (sem ele, vale o nome do arquivo).
2. Linhas `chave: valor` entre o `# ` e o primeiro `## ` são metadados da suíte.
3. Cada `## ` abre um cenário. Comece pelo id (`SC-01`, `CT-7`, `REG-102`)
   seguido de `·`, `-`, `—`, `:` ou espaço. Sem id explícito, o JAFIZ gera um
   pela posição — e um cenário inserido no meio desloca os ids abaixo dele.
4. Sob o `## `, antes do primeiro passo: `tags: a, b, c` e
   `pré: ...` (ou `pre:`, `precondição:`, `precondition:`, `given:`).
5. Texto solto antes do primeiro passo vira a descrição do cenário.
6. Cada item de lista (`1.`, `1)`, `-`, `*`) é um passo. A numeração do arquivo
   é ignorada — os passos são renumerados a partir de 1 em cada cenário.
7. Dentro do passo, o primeiro `→`, `->` ou `=>` separa a **ação** do
   **resultado esperado**.
8. Linha indentada (2+ espaços) logo após um passo continua esse passo.
9. Linhas em branco, `<!-- -->`, `>` e `---` são ignoradas.
10. UTF-8.

## Exemplo

```markdown
# Checkout — regressão v2.3
produto: Loja Tolky

## SC-01 · Compra com cartão aprovado
tags: checkout, pagamento
pré: usuário logado; carrinho com 1 item

1. Abrir o carrinho → o item aparece com preço correto
2. Clicar em "Finalizar compra" → a tela de pagamento abre
3. Informar o cartão de teste 4111 1111 1111 1111
   → o botão "Pagar" habilita
```

## Fluxo

- Escreva o `.md` e rode `jafiz check <arquivo>` para validar.
- O usuário executa os passos no `jafiz-gui` e marca cada um.
- Rode `jafiz report` para ler o que passou, o que falhou e as observações.
