package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"flag"
	"fmt"
	"go/format"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

type endpoint struct {
	Operation      string
	Method         string
	Path           string
	Authentication string
	Mutation       bool
}

func main() {
	check := flag.Bool("check", false, "fail if the generated endpoint map has drifted")
	flag.Parse()

	root, err := repositoryRoot()
	must(err)
	command := exec.Command(
		"cargo", "run", "-q", "--manifest-path", filepath.Join(root, "Cargo.toml"),
		"-p", "rrd-contract", "--bin", "rrd-contract-export",
	)
	raw, err := command.Output()
	must(err)
	generated, err := render(raw)
	must(err)
	output := filepath.Join(root, "sdks", "go", "endpoints_gen.go")
	if *check {
		current, readErr := os.ReadFile(output)
		if readErr != nil || !bytes.Equal(current, generated) {
			fmt.Fprintln(os.Stderr, "generated Go RRD endpoint map is stale; run go run ./cmd/generate")
			os.Exit(1)
		}
		return
	}
	must(os.WriteFile(output, generated, 0o644))
}

func repositoryRoot() (string, error) {
	directory, err := os.Getwd()
	if err != nil {
		return "", err
	}
	for {
		if _, statErr := os.Stat(filepath.Join(directory, "crates", "rrd-contract")); statErr == nil {
			return directory, nil
		}
		parent := filepath.Dir(directory)
		if parent == directory {
			return "", fmt.Errorf("could not locate repository root")
		}
		directory = parent
	}
}

func render(raw []byte) ([]byte, error) {
	var document struct {
		Paths map[string]map[string]struct {
			OperationID string                   `json:"operationId"`
			Security    []map[string]interface{} `json:"security"`
			Mutation    bool                     `json:"x-rrd-mutation"`
		} `json:"paths"`
	}
	if err := json.Unmarshal(raw, &document); err != nil {
		return nil, err
	}
	endpoints := make([]endpoint, 0, len(document.Paths))
	for path, pathItem := range document.Paths {
		for method, operation := range pathItem {
			if operation.OperationID == "" {
				continue
			}
			authentication := "public"
			if len(operation.Security) > 0 {
				for scheme := range operation.Security[0] {
					switch scheme {
					case "rrdApiKey":
						authentication = "api_key"
					case "rrdBearer":
						authentication = "session_bearer"
					}
				}
			}
			endpoints = append(endpoints, endpoint{
				Operation:      operation.OperationID,
				Method:         strings.ToUpper(method),
				Path:           path,
				Authentication: authentication,
				Mutation:       operation.Mutation,
			})
		}
	}
	sort.Slice(endpoints, func(left, right int) bool {
		return endpoints[left].Operation < endpoints[right].Operation
	})
	var source strings.Builder
	source.WriteString("// Code generated from rrd-contract; DO NOT EDIT.\n\npackage rrd\n\n")
	digest := sha256.Sum256(raw)
	fmt.Fprintf(&source, "// OpenAPI SHA-256: %x\n\n", digest)
	source.WriteString("type OperationID string\n\nconst (\n")
	for _, item := range endpoints {
		fmt.Fprintf(&source, "\tOperation%s OperationID = %q\n", exportedName(item.Operation), item.Operation)
	}
	source.WriteString(")\n\nvar endpoints = map[OperationID]Endpoint{\n")
	for _, item := range endpoints {
		fmt.Fprintf(
			&source,
			"\tOperation%s: {Method: %q, Path: %q, Authentication: %q, Mutation: %t},\n",
			exportedName(item.Operation), item.Method, item.Path, item.Authentication, item.Mutation,
		)
	}
	source.WriteString("}\n")
	formatted, err := format.Source([]byte(source.String()))
	if err != nil {
		return nil, fmt.Errorf("format generated source: %w", err)
	}
	return formatted, nil
}

func exportedName(value string) string {
	var result strings.Builder
	for _, part := range strings.Split(value, "-") {
		if part == "" {
			continue
		}
		result.WriteString(strings.ToUpper(part[:1]))
		result.WriteString(part[1:])
	}
	return result.String()
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
