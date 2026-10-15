from typing import Literal

AddRuntimeRequestGithubKind = Literal["github"]

ADD_RUNTIME_REQUEST_GITHUB_KIND_VALUES: set[AddRuntimeRequestGithubKind] = {
    "github",
}


def check_add_runtime_request_github_kind(value: str) -> AddRuntimeRequestGithubKind:
    if value in ADD_RUNTIME_REQUEST_GITHUB_KIND_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {ADD_RUNTIME_REQUEST_GITHUB_KIND_VALUES!r}"
    )
