from typing import Literal

SearchProviderKind = Literal["brave", "duckduckgo", "none", "searxng", "tavily"]

SEARCH_PROVIDER_KIND_VALUES: set[SearchProviderKind] = {
    "brave",
    "duckduckgo",
    "none",
    "searxng",
    "tavily",
}


def check_search_provider_kind(value: str) -> SearchProviderKind:
    if value in SEARCH_PROVIDER_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SEARCH_PROVIDER_KIND_VALUES!r}")
