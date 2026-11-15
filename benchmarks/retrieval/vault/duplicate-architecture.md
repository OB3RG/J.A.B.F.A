# Archived agent architecture

## Recall workflow

The brain agents were slow because every recall request made an LLM scan directories and reread complete catalogs. Repeated filesystem crawling spent tokens on irrelevant notes before the useful decision record was reached.
