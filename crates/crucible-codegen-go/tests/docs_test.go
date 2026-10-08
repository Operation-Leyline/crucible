package docs

import (
    "fmt"
    "strings"
    "testing"
)

func TestRecursiveFormattingRedactsMembersAndTargets(t *testing.T) {
    if CodeONE.String() != "1" || StatusREADY.String() != `"ready"` { t.Fatal("enum formatting") }
    child := &Record{Label: Some("child"), Secret: Some("member-secret"), Token: Some("target-secret")}
    record := Record{Label: Some("visible"), Children: Some(Records{child}), Lookup: Some(RecordMap{"child":child}), Choice: Some(&Choice{Secret: Some("union-secret")}), Hidden: Some(&Hidden{Value: Some("hidden-secret")})}
    for _, text := range []string{record.String(), fmt.Sprintf("%+v", record), fmt.Sprintf("%#v", &record)} {
        for _, secret := range []string{"member-secret","target-secret","union-secret","hidden-secret"} {
            if strings.Contains(text,secret) { t.Fatalf("sensitive value leaked: %s",text) }
        }
        if !strings.Contains(text,"Record(") || !strings.Contains(text,"*** Sensitive Data Redacted ***") || strings.Contains(text,"Code=") { t.Fatal(text) }
    }
    record.Label = Null[string]()
    if !strings.Contains(record.String(),"Label=null") { t.Fatal(record.String()) }
    child.Children = Some(Records{child})
    if !strings.Contains(child.String(),"<recursive>") { t.Fatal(child.String()) }
    if strings.Contains(fmt.Sprintf("%+v", Records{child}),"member-secret") { t.Fatal("list leaked") }
    error := &Problem{Message: Some("message-secret"), Record: Some(child), Token: Some("error-secret")}
    for _, text := range []string{error.String(), error.Error(), fmt.Sprintf("%+v", error), fmt.Sprintf("%#v", error)} {
        for _, secret := range []string{"message-secret","error-secret","member-secret","target-secret"} {
            if strings.Contains(text,secret) { t.Fatal(text) }
        }
    }
    unknown := Choice{UnknownVariant:&UnknownUnion{Name:"future",Value:"unknown-secret"}}
    if strings.Contains(unknown.String(),"unknown-secret") { t.Fatal(unknown.String()) }
}
