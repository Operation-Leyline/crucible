"""Bearer authentication for generated clients."""
from __future__ import annotations

import httpx


def preset_auth(request: httpx.Request) -> bool:
    """Whether the request already carries a bearer credential, which then wins over the configured token."""
    value = request.headers.get("authorization", "")
    present = value.lower().startswith("bearer ") and len(value) > len("bearer ")
    if present:
        request.extensions["crucible.credentials"] = True
    return present


def apply_auth(request: httpx.Request, token: object) -> None:
    """Send the token as a bearer credential."""
    request.headers["authorization"] = f"Bearer {token}"
    request.extensions["crucible.credentials"] = True
