"""Weather plus isolated operations for shared retry qualification."""
import re

from sdk_cru_packages import add_operations, copy_package, edit_files

GLOBAL_ERRORS = "ThrottlingError, ValidationError"

EXTRA = """error Ordinary {
    status 400
    message?: string
}

error Transient {
    status 400
    retryable
    message?: string
}

error Throttled {
    status 429
    retryable throttling
    message?: string
}

idempotent operation RetrySafe {
    http POST "/retry/safe"
    errors [Ordinary, Transient, Throttled]
}

operation RetryUnsafe {
    http POST "/retry/unsafe"
    errors [Ordinary, Transient, Throttled]
}

public readonly operation PublicPing {
    http GET "/public" -> 204
}
"""


def qualification_model(output):
    """The Weather `.cru` package with the retry operations added; returns its directory.

    The service's own errors move onto each Weather operation, so the isolated
    operations see only the errors they declare."""
    package = copy_package("weather", output / "model")

    def edit(path, text):
        if path.name == "weather.cru":
            text = text.replace(f"    errors [{GLOBAL_ERRORS}]\n", "")
        elif "operations" in path.parts:
            if re.search(r"\n    errors \[", text):
                text = re.sub(r"(\n    errors \[)([^\]]*)\]", lambda m: m.group(1) + m.group(2) + ", " + GLOBAL_ERRORS + "]", text, count=1)
            else:
                text = re.sub(r"(\n    http [^\n]*\n)", lambda m: m.group(1) + f"    errors [{GLOBAL_ERRORS}]\n", text, count=1)
        return text

    edit_files(package, edit)
    (package / "conformance.cru").write_text(EXTRA, encoding="utf-8", newline="\n")
    add_operations(package, "RetrySafe", "RetryUnsafe", "PublicPing")
    return package
