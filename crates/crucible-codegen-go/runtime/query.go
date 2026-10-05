package sdk

import (
    "encoding/json"
    "errors"
    "math/big"
    "sort"
    "strconv"
    "strings"
    "unicode/utf8"
)

type queryFailure struct { message string }
func qfail(message string) { panic(queryFailure{message}) }
func qtext(value any) string { if text, ok := value.(string); ok { return text }; qfail("JMESPath expected string"); return "" }
func qarray(value any) []any { if list, ok := value.([]any); ok { return list }; qfail("JMESPath expected array"); return nil }
func qobject(value any) map[string]any { if object, ok := value.(map[string]any); ok { return object }; qfail("JMESPath expected object"); return nil }
func qnumber(value any) json.Number { if number, ok := value.(json.Number); ok { return number }; qfail("JMESPath expected number"); return "" }
func qinteger(value any) int { number, err := strconv.Atoi(string(qnumber(value))); if err != nil { qfail("JMESPath integer out of range") }; return number }
func qtruth(value any) bool {
    switch v := value.(type) { case nil: return false; case bool: return v; case string: return v != ""; case []any: return len(v) != 0; case map[string]any: return len(v) != 0; default: return true }
}
func qcompare(left, right any) int {
    a, numeric := left.(json.Number)
    if !numeric { return strings.Compare(qtext(left),qtext(right)) }
    b := qnumber(right)
    ac, ae := decimalParts(string(a)); bc, be := decimalParts(string(b))
    sign := func(text string) int { if text == "0" { return 0 }; if strings.HasPrefix(text,"-") { return -1 }; return 1 }
    as, bs := sign(ac), sign(bc); if as < bs { return -1 }; if as > bs { return 1 }; if as == 0 { return 0 }
    ac, bc = strings.TrimPrefix(ac,"-"),strings.TrimPrefix(bc,"-")
    ax,_ := new(big.Int).SetString(ae,10); bx,_ := new(big.Int).SetString(be,10)
    ax.Add(ax,big.NewInt(int64(len(ac)))); bx.Add(bx,big.NewInt(int64(len(bc))))
    if order := ax.Cmp(bx); order != 0 { return as * order }
    count := max(len(ac),len(bc)); ac += strings.Repeat("0",count-len(ac)); bc += strings.Repeat("0",count-len(bc))
    return as * strings.Compare(ac,bc)
}

// queryEvaluate consumes the portable tree produced by crucible-smithy.
func queryEvaluate(node map[string]any, input any) (result any, err error) {
    defer func(){ if failure := recover(); failure != nil { if value, ok := failure.(queryFailure); ok { err = errors.New(value.message) } else { panic(failure) } } }()
    return qeval(node,input),nil
}
func qeval(node map[string]any, input any) any {
    eval := func(name string) any { return qeval(qobject(node[name]),input) }
    switch qtext(node["kind"]) {
    case "identity": return input
    case "literal": return node["value"]
    case "field": if object, ok := input.(map[string]any); ok { return object[qtext(node["name"])] }; return nil
    case "index":
        list, ok := input.([]any); if !ok { return nil }; index := qinteger(node["index"]); if index < 0 { index += len(list) }; if index < 0 || index >= len(list) { return nil }; return list[index]
    case "subexpression": return qeval(qobject(node["right"]),eval("left"))
    case "projection":
        list, ok := eval("left").([]any); if !ok { return nil }; result := []any{}
        for _, item := range list { value := qeval(qobject(node["right"]),item); if value != nil { result = append(result,value) } }; return result
    case "flatten":
        list, ok := eval("expression").([]any); if !ok { return nil }; result := []any{}
        for _, item := range list { if nested, ok := item.([]any); ok { result = append(result,nested...) } else { result = append(result,item) } }; return result
    case "objectValues":
        object, ok := eval("expression").(map[string]any); if !ok { return nil }; return qvalues(object)
    case "multiList":
        if input == nil { return nil }; result := []any{}; for _, item := range qarray(node["expressions"]) { result = append(result,qeval(qobject(item),input)) }; return result
    case "multiHash":
        if input == nil { return nil }; result := map[string]any{}; for _, item := range qarray(node["entries"]) { entry := qobject(item); result[qtext(entry["key"])] = qeval(qobject(entry["value"]),input) }; return result
    case "and": left := eval("left"); if qtruth(left) { return eval("right") }; return left
    case "or": left := eval("left"); if qtruth(left) { return left }; return eval("right")
    case "not": return !qtruth(eval("expression"))
    case "condition": if qtruth(eval("predicate")) { return eval("then") }; return nil
    case "compare":
        left, right := eval("left"),eval("right"); comparator := qtext(node["comparator"])
        if comparator == "equal" { return wireEqual(left,right) }; if comparator == "notEqual" { return !wireEqual(left,right) }
        if _, ok := left.(json.Number); !ok { return nil }; if _, ok := right.(json.Number); !ok { return nil }; order := qcompare(left,right)
        switch comparator { case "lessThan": return order < 0; case "lessThanEqual": return order <= 0; case "greaterThan": return order > 0; case "greaterThanEqual": return order >= 0 }; qfail("Unknown JMESPath comparator")
    case "slice":
        list, ok := input.([]any); if !ok { return nil }; step := qinteger(node["step"]); if step == 0 { qfail("Zero slice step") }
        boundary := func(name string, fallback int) int { if node[name] == nil { return fallback }; value := qinteger(node[name]); if value < 0 { value += len(list) }; if step > 0 { return min(len(list),max(0,value)) }; return min(len(list)-1,max(-1,value)) }
        start,stop := 0,len(list); if step < 0 { start,stop = len(list)-1,-1 }; start,stop = boundary("start",start),boundary("stop",stop)
        result := []any{}; for i := start; (step > 0 && i < stop) || (step < 0 && i > stop); i += step { result = append(result,list[i]) }; return result
    case "function": return qfunction(qtext(node["function"]),qarray(node["arguments"]),input)
    }
    qfail("Unknown compiled JMESPath node"); return nil
}
func qkeys(object map[string]any) []string { keys := make([]string,0,len(object)); for key := range object { keys = append(keys,key) }; sort.Strings(keys); return keys }
func qvalues(object map[string]any) []any { result := []any{}; for _, key := range qkeys(object) { result = append(result,object[key]) }; return result }
func qordered(value any) []any {
    result := append([]any{},qarray(value)...)
    if len(result) > 0 { _, numeric := result[0].(json.Number); for _, item := range result { if numeric { qnumber(item) } else { qtext(item) } } }
    sort.SliceStable(result,func(i,j int)bool{ return qcompare(result[i],result[j]) < 0 }); return result
}
func qreference(nodes []any, index int, input any) any { node := qobject(nodes[index]); if qtext(node["kind"]) != "expressionReference" { qfail("JMESPath expected expression reference") }; return qeval(qobject(node["expression"]),input) }
func qrational(value any) *big.Rat {
    text := string(qnumber(value)); coefficient, exp := decimalParts(text); exponent,_ := new(big.Int).SetString(exp,10)
    if new(big.Int).Abs(exponent).Cmp(big.NewInt(1_000_000)) > 0 { qfail("JMESPath arithmetic exponent exceeds supported precision") }
    result,ok := new(big.Rat).SetString(coefficient+"e"+exp); if !ok { qfail("Invalid JMESPath number") }; return result
}
func qresult(value *big.Rat, precision int) json.Number {
    if value.IsInt() { return json.Number(value.Num().String()) }
    text := strings.TrimRight(strings.TrimRight(value.FloatString(precision),"0"),"."); if text == "-0" { text = "0" }; return json.Number(text)
}
func qfunction(name string, nodes []any, input any) any {
    variadic := name == "merge" || name == "not_null"; arity := 1
    switch name { case "contains","starts_with","ends_with","join","map","max_by","min_by","sort_by": arity = 2 }
    if (variadic && len(nodes) == 0) || (!variadic && len(nodes) != arity) { qfail("JMESPath invalid function arity") }
    arg := func(index int) any { return qeval(qobject(nodes[index]),input) }
    switch name {
    case "abs": return json.Number(strings.TrimPrefix(string(qnumber(arg(0))),"-"))
    case "ceil","floor":
        value := qrational(arg(0)); quotient,remainder := new(big.Int),new(big.Int); quotient.QuoRem(value.Num(),value.Denom(),remainder)
        if remainder.Sign() != 0 && ((name == "ceil" && value.Sign() > 0) || (name == "floor" && value.Sign() < 0)) { delta := int64(1); if name == "floor" { delta = -1 }; quotient.Add(quotient,big.NewInt(delta)) }; return json.Number(quotient.String())
    case "sum","avg":
        list := qarray(arg(0)); total := new(big.Rat); for _, item := range list { total.Add(total,qrational(item)) }
        if name == "avg" { if len(list) == 0 { return nil }; total.Quo(total,new(big.Rat).SetInt64(int64(len(list)))); return qresult(total,34) }
        // Decimal sums have a terminating base-ten representation.
        denominator := new(big.Int).Set(total.Denom()); two,five := 0,0; remainder := new(big.Int)
        for denominator.Sign() > 0 && new(big.Int).Mod(denominator,big.NewInt(2)).Sign() == 0 { denominator.QuoRem(denominator,big.NewInt(2),remainder); two++ }
        for denominator.Sign() > 0 && new(big.Int).Mod(denominator,big.NewInt(5)).Sign() == 0 { denominator.QuoRem(denominator,big.NewInt(5),remainder); five++ }
        return qresult(total,max(two,five))
    case "contains": source,wanted := arg(0),arg(1); if text, ok := source.(string); ok { return strings.Contains(text,qtext(wanted)) }; for _, item := range qarray(source) { if wireEqual(item,wanted) { return true } }; return false
    case "starts_with": return strings.HasPrefix(qtext(arg(0)),qtext(arg(1)))
    case "ends_with": return strings.HasSuffix(qtext(arg(0)),qtext(arg(1)))
    case "join": values := []string{}; for _, item := range qarray(arg(1)) { values = append(values,qtext(item)) }; return strings.Join(values,qtext(arg(0)))
    case "keys": result := []any{}; for _, key := range qkeys(qobject(arg(0))) { result = append(result,key) }; return result
    case "values": return qvalues(qobject(arg(0)))
    case "length": value := arg(0); var length int; switch v := value.(type) { case []any: length = len(v); case map[string]any: length = len(v); default: length = utf8.RuneCountInString(qtext(value)) }; return json.Number(strconv.Itoa(length))
    case "map": result := []any{}; for _, item := range qarray(arg(1)) { result = append(result,qreference(nodes,0,item)) }; return result
    case "max","min","sort": list := qordered(arg(0)); if name == "sort" { return list }; if len(list) == 0 { return nil }; if name == "max" { return list[len(list)-1] }; return list[0]
    case "max_by","min_by","sort_by":
        list := qarray(arg(0)); keys := []any{}; for _, item := range list { keys = append(keys,qreference(nodes,1,item)) }; qordered(keys)
        if len(list) == 0 { if name == "sort_by" { return []any{} }; return nil }
        if name == "sort_by" { indices := make([]int,len(list)); for i := range indices { indices[i] = i }; sort.SliceStable(indices,func(i,j int)bool{ return qcompare(keys[indices[i]],keys[indices[j]]) < 0 }); result := []any{}; for _, index := range indices { result = append(result,list[index]) }; return result }
        selected := 0; for i := 1; i < len(list); i++ { order := qcompare(keys[i],keys[selected]); if (name == "max_by" && order > 0) || (name == "min_by" && order < 0) { selected = i } }; return list[selected]
    case "merge": result := map[string]any{}; for i := range nodes { for key,value := range qobject(arg(i)) { result[key] = value } }; return result
    case "not_null": for i := range nodes { if value := arg(i); value != nil { return value } }; return nil
    case "reverse":
        value := arg(0); if list, ok := value.([]any); ok { result := append([]any{},list...); for i,j := 0,len(result)-1; i < j; i,j = i+1,j-1 { result[i],result[j] = result[j],result[i] }; return result }
        points := []rune(qtext(value)); for i,j := 0,len(points)-1; i < j; i,j = i+1,j-1 { points[i],points[j] = points[j],points[i] }; return string(points)
    case "to_array": value := arg(0); if _, ok := value.([]any); ok { return value }; return []any{value}
    case "to_number": value := arg(0); if _, ok := value.(json.Number); ok { return value }; if text, ok := value.(string); ok { if _,err := ParseDecimal(text); err == nil { return json.Number(text) } }; return nil
    case "to_string": value := arg(0); if _, ok := value.(string); ok { return value }; data,err := json.Marshal(value); if err != nil { qfail(err.Error()) }; return string(data)
    case "type": switch arg(0).(type) { case nil: return "null"; case bool: return "boolean"; case string: return "string"; case json.Number: return "number"; case []any: return "array"; case map[string]any: return "object" }; qfail("Unknown JMESPath value")
    }
    qfail("Unknown compiled JMESPath function"); return nil
}
