package main
import (
"context"
"log"
"github.com/hashicorp/terraform-plugin-framework/providerserver"
"example.com/terraform-provider-inventory/internal/provider"
)
func main() { if err := providerserver.Serve(context.Background(), provider.New, providerserver.ServeOpts{Address: "registry.terraform.io/example/inventory", ProtocolVersion: 6}); err != nil { log.Fatal(err) } }
