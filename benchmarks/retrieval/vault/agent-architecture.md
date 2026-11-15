# Agent architecture

## Recall workflow

The brain agents were slow because every recall request made an LLM scan directories and reread complete catalogs. Repeated filesystem crawling spent tokens on irrelevant notes before the useful decision record was reached.

## Implementation constraint

Keep retrieval deterministic and local. Software should find candidate passages and supply citations rather than asking an agent to rediscover the vault structure for every question.
