"""Adapt the shared, compiled JMESPath IR to the pinned native interpreter."""
from decimal import Decimal, InvalidOperation
import json
from numbers import Number
from typing import Any

from jmespath import functions, visitor  # type: ignore[import-untyped]


def equal(left: Any, right: Any) -> bool:
    """Compare JSON-like values using JMESPath numeric equality while keeping booleans distinct from numbers."""
    if isinstance(left, dict) and isinstance(right, dict):
        return left.keys() == right.keys() and all(equal(value, right[key]) for key, value in left.items())
    if isinstance(left, list) and isinstance(right, list):
        return len(left) == len(right) and all(equal(a, b) for a, b in zip(left, right))
    if isinstance(left, Number) and not isinstance(left, bool):
        return isinstance(right, Number) and not isinstance(right, bool) and left == right
    return type(left) is type(right) and left == right


def json_text(value: Any) -> str:
    """Serialize JSON-like values while preserving exact Decimal numbers."""
    if isinstance(value, Decimal):
        if not value.is_finite():
            raise ValueError("nonfinite JSON number")
        return str(value)
    if isinstance(value, list):
        return "[" + ",".join(json_text(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(key) + ":" + json_text(item) for key, item in value.items()) + "}"
    return json.dumps(value, allow_nan=False, separators=(",", ":"))


class Functions(functions.Functions):
    """JMESPath functions extended for exact Decimal values."""
    def _get_allowed_pytypes(self, types: Any) -> Any:
        allowed, nested = super()._get_allowed_pytypes(types)
        if "int" in allowed:
            allowed.append("Decimal")
        nested = [values + ("Decimal",) if "int" in values else values for values in nested]
        return allowed, nested

    def _convert_to_jmespath_type(self, name: str) -> str:
        return "number" if name == "Decimal" else super()._convert_to_jmespath_type(name)

    @functions.signature({"types": []})
    def _func_to_number(self, value: Any) -> Any:
        if isinstance(value, bool) or value is None or isinstance(value, (list, dict)):
            return None
        if isinstance(value, Number):
            return value
        try:
            number = Decimal(value)
            return number if number.is_finite() else None
        except (InvalidOperation, TypeError, ValueError):
            return None

    @functions.signature({"types": []})
    def _func_to_string(self, value: Any) -> str:
        return value if isinstance(value, str) else json_text(value)

    @functions.signature({"types": []})
    def _func_type(self, value: Any) -> str:
        return "number" if isinstance(value, Decimal) else super()._func_type(value)

    @functions.signature({"types": ["array-number"]})
    def _func_avg(self, values: Any) -> Any:
        return sum(values) / len(values) if values else None

    @functions.signature({"types": ["array", "string"]}, {"types": []})
    def _func_contains(self, values: Any, value: Any) -> bool:
        return value in values if isinstance(values, str) else any(equal(item, value) for item in values)


class Interpreter(visitor.TreeInterpreter):
    """JMESPath interpreter preserving numeric and object-value semantics of the lowered expressions."""
    def visit_condition(self, node: Any, value: Any) -> Any:
        return self.visit(node["children"][1], value) if self._is_true(self.visit(node["children"][0], value)) else None

    def visit_object_values(self, node: Any, value: Any) -> Any:
        result = self.visit(node["children"][0], value)
        return list(result.values()) if isinstance(result, dict) else None

    def visit_comparator(self, node: Any, value: Any) -> Any:
        left, right = (self.visit(child, value) for child in node["children"])
        kind = node["value"]
        if kind == "eq":
            return equal(left, right)
        if kind == "ne":
            return not equal(left, right)
        if any(isinstance(item, bool) or not isinstance(item, Number) for item in (left, right)):
            return None
        return self.COMPARATOR_FUNC[kind](left, right)


def ast(expression: dict[str, Any]) -> dict[str, Any]:
    """Convert lowered expression IR to the pinned native JMESPath interpreter AST."""
    kind = expression["kind"]
    simple = {"identity": "identity", "field": "field", "index": "index", "literal": "literal"}
    if kind in simple:
        return {"type": simple[kind], "children": [], "value": expression.get("name", expression.get("index", expression.get("value")))}
    binary = {"subexpression": "subexpression", "projection": "projection", "and": "and_expression", "or": "or_expression"}
    if kind in binary:
        return {"type": binary[kind], "children": [ast(expression["left"]), ast(expression["right"])]}
    unary = {"flatten": "flatten", "objectValues": "object_values", "not": "not_expression", "expressionReference": "expref"}
    if kind in unary:
        return {"type": unary[kind], "children": [ast(expression["expression"])]}
    if kind == "slice":
        return {"type": "slice", "children": [expression["start"], expression["stop"], expression["step"]]}
    if kind == "multiList":
        return {"type": "multi_select_list", "children": [ast(item) for item in expression["expressions"]]}
    if kind == "multiHash":
        return {"type": "multi_select_dict", "children": [{"type": "key_val_pair", "value": item["key"], "children": [ast(item["value"])]} for item in expression["entries"]]}
    if kind == "compare":
        comparator = {"equal": "eq", "notEqual": "ne", "lessThan": "lt", "lessThanEqual": "lte", "greaterThan": "gt", "greaterThanEqual": "gte"}[expression["comparator"]]
        return {"type": "comparator", "value": comparator, "children": [ast(expression["left"]), ast(expression["right"])]}
    if kind == "condition":
        return {"type": "condition", "children": [ast(expression["predicate"]), ast(expression["then"])]}
    if kind == "function":
        return {"type": "function_expression", "value": expression["function"], "children": [ast(item) for item in expression["arguments"]]}
    raise ValueError(f"unknown compiled JMESPath expression {kind}")


class Query:
    """Compile a lowered JMESPath expression once and evaluate it against model values."""
    def __init__(self, expression: dict[str, Any]) -> None:
        self._ast = ast(expression)
        self._interpreter = Interpreter(visitor.Options(custom_functions=Functions()))

    def search(self, value: Any) -> Any:
        """Evaluate the compiled expression against a model value."""
        return self._interpreter.visit(self._ast, value)
