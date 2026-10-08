import pydoc
import unittest
import httpx
from docs_client import DocsClient, AsyncDocsClient, Record, Hidden, Choice, Problem, ResponseMetadata


class DocumentationTests(unittest.TestCase):
    def test_recursive_repr_redacts_members_and_targets(self):
        child = Record(label="child", secret="member-secret", token="target-secret")
        record = Record(label="visible", children=[child], lookup={"child": child},
                        choice=Choice(secret="union-secret"), hidden=Hidden(value="hidden-secret"))
        text = repr(record)
        self.assertIn("Record(", text)
        self.assertIn("label='visible'", text)
        self.assertIn("*** Sensitive Data Redacted ***", text)
        self.assertIn("children=[Record(", text)
        self.assertNotIn("member-secret", text)
        self.assertNotIn("target-secret", text)
        self.assertNotIn("union-secret", text)
        self.assertNotIn("hidden-secret", text)
        self.assertNotIn("code=", text)
        self.assertIn("label=None", repr(Record(label=None)))
        child.children = [child]
        self.assertIn("<recursive>", repr(child))

    def test_errors_and_unknown_union_payloads_are_safe(self):
        error = Problem(name="Problem", message="message-secret", metadata=ResponseMetadata(400, None, httpx.Headers()),
                        fields={"record": Record(token="target-secret"), "token": "error-secret"})
        for text in (repr(error), str(error)):
            for secret in ("message-secret", "target-secret", "error-secret"):
                self.assertNotIn(secret, text)
            self.assertIn("*** Sensitive Data Redacted ***", text)
        self.assertNotIn("unknown-secret", repr(Choice(_unknown=("future", "unknown-secret"))))

    def test_native_documentation_renders(self):
        for client in (DocsClient, AsyncDocsClient):
            text = pydoc.render_doc(client)
            self.assertIn("https://example.com", text)
            self.assertIn("max_attempts", client.__init__.__doc__)
            self.assertIn("Modeled errors: Problem", client.inspect.__doc__)
        self.assertIn("Readable link (https://example.com)", Record.__doc__)
        self.assertIn("line two", Record.__doc__)
