# Exemplo JAFIZ — todas as regras do formato
produto: Foundry32
url: https://github.com/atlas-jedi/foundry32

<!-- Este arquivo é a fixture de verificação do parser. -->

## SC-01 · Cenário com id explícito
tags: exemplo, parser
pré: o app está aberto

Um parágrafo solto vira a descrição do cenário.

1. Abrir a suíte de exemplo → a lista mostra quatro cenários
2. Selecionar o primeiro cenário -> os passos aparecem à direita
3. Conferir o rodapé => o progresso mostra 0 de N passos
4. Um passo sem resultado esperado

## SC-02 - Separador de traço no título
tags: exemplo

1. Um passo cuja ação é longa o bastante para justificar
   uma segunda linha → e o esperado começa aqui
2. Outro passo → outro esperado

## Cenário sem id explícito
Este recebe um id gerado a partir da posição.

- Passo escrito com marcador → também é um passo
- Segundo passo com marcador → segundo esperado

## SC-04: Separador de dois-pontos
1. Passo único → resultado único
