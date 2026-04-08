package main

import (
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"testing"
)

func TestBuildQueryMemoryInvocationUsesDefaultProjectInJSONMode(t *testing.T) {
	oldEnv := os.Getenv("MEMORY_LAYER_PROJECT")
	t.Cleanup(func() { _ = os.Setenv("MEMORY_LAYER_PROJECT", oldEnv) })
	if err := os.Setenv("MEMORY_LAYER_PROJECT", "mem-project"); err != nil {
		t.Fatal(err)
	}

	got, err := buildQueryMemoryInvocation([]string{"How does this repo work?"}, outputJSON)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	want := []string{"query", "--project", "mem-project", "--question", "How does this repo work?", "--json"}
	if len(got.commandArgs) != len(want) {
		t.Fatalf("unexpected arg count: got %v want %v", got, want)
	}
	for i := range want {
		if got.commandArgs[i] != want[i] {
			t.Fatalf("arg %d mismatch: got %q want %q", i, got.commandArgs[i], want[i])
		}
	}
	if got.project != "mem-project" {
		t.Fatalf("unexpected project: %q", got.project)
	}
}

func TestBuildCheckpointProjectInvocationAddsJSONByDefault(t *testing.T) {
	got, err := buildCheckpointProjectInvocation([]string{"--project", "memory", "--note", "Plan approved"}, outputJSON)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	want := []string{"checkpoint", "save", "--project", "memory", "--note", "Plan approved", "--json"}
	if len(got.commandArgs) != len(want) {
		t.Fatalf("unexpected arg count: got %v want %v", got, want)
	}
	for i := range want {
		if got.commandArgs[i] != want[i] {
			t.Fatalf("arg %d mismatch: got %q want %q", i, got.commandArgs[i], want[i])
		}
	}
}

func TestBuildCaptureTaskArgsRejectsMissingFile(t *testing.T) {
	_, err := buildCaptureTaskInvocation([]string{"/tmp/does-not-exist.json"})
	if err == nil {
		t.Fatal("expected missing file error")
	}
}

func TestParseGlobalOptionsSupportsTextMode(t *testing.T) {
	mode, remaining, err := parseGlobalOptions([]string{"--text", "resume-project", "memory"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mode != outputText {
		t.Fatalf("expected text mode, got %v", mode)
	}
	if len(remaining) != 2 || remaining[0] != "resume-project" || remaining[1] != "memory" {
		t.Fatalf("unexpected remaining args: %v", remaining)
	}
}

func TestEmitHelperSuccessUsesStructuredEnvelope(t *testing.T) {
	oldStdout := os.Stdout
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	os.Stdout = w
	t.Cleanup(func() { os.Stdout = oldStdout })

	exitCode := emitHelperSuccess("resume-project", "memory", map[string]any{"answer": "ok"})
	_ = w.Close()
	output, _ := io.ReadAll(r)

	if exitCode != 0 {
		t.Fatalf("unexpected exit code: %d", exitCode)
	}
	var envelope helperEnvelope
	if err := json.Unmarshal(output, &envelope); err != nil {
		t.Fatalf("invalid json output: %v\n%s", err, string(output))
	}
	if !envelope.Ok || envelope.HelperCommand != "resume-project" || envelope.Project != "memory" {
		t.Fatalf("unexpected envelope: %+v", envelope)
	}
}

func TestEmitHelperFailureUsesStructuredEnvelope(t *testing.T) {
	oldStdout := os.Stdout
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	os.Stdout = w
	t.Cleanup(func() { os.Stdout = oldStdout })

	exitCode := emitHelperFailure(outputJSON, "query-memory", "memory", "command_failed", "boom", 7, []string{"memory", "query"}, "partial", "boom")
	_ = w.Close()
	output, _ := io.ReadAll(r)

	if exitCode != 7 {
		t.Fatalf("unexpected exit code: %d", exitCode)
	}
	var envelope helperEnvelope
	if err := json.Unmarshal(output, &envelope); err != nil {
		t.Fatalf("invalid json output: %v\n%s", err, string(output))
	}
	if envelope.Ok || envelope.Error == nil || envelope.Error.Kind != "command_failed" {
		t.Fatalf("unexpected envelope: %+v", envelope)
	}
	if envelope.Error.ExitCode == nil || *envelope.Error.ExitCode != 7 {
		t.Fatalf("missing exit code in envelope: %+v", envelope)
	}
}

func TestResolverPrefersExplicitMemctlBin(t *testing.T) {
	oldEnv := os.Getenv("MEMCTL_BIN")
	t.Cleanup(func() { _ = os.Setenv("MEMCTL_BIN", oldEnv) })
	if err := os.Setenv("MEMCTL_BIN", "memory --json"); err != nil {
		t.Fatal(err)
	}

	resolver := &memoryCommandResolver{sourceRoot: t.TempDir()}
	got, err := resolver.Resolve()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	want := []string{"memory", "--json"}
	if len(got) != len(want) {
		t.Fatalf("unexpected arg count: got %v want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("arg %d mismatch: got %q want %q", i, got[i], want[i])
		}
	}
}

func TestResolverFallsBackToCargoWhenSourceRepoExists(t *testing.T) {
	sourceRoot := t.TempDir()
	mustWriteFile(t, filepath.Join(sourceRoot, "Cargo.toml"), []byte("[workspace]\n"))
	mustWriteFile(t, filepath.Join(sourceRoot, "crates", "mem-cli", "Cargo.toml"), []byte("[package]\nname=\"mem-cli\"\n"))

	oldEnv := os.Getenv("MEMCTL_BIN")
	t.Cleanup(func() { _ = os.Setenv("MEMCTL_BIN", oldEnv) })
	if err := os.Unsetenv("MEMCTL_BIN"); err != nil {
		t.Fatal(err)
	}
	oldPath := os.Getenv("PATH")
	t.Cleanup(func() { _ = os.Setenv("PATH", oldPath) })
	if err := os.Setenv("PATH", ""); err != nil {
		t.Fatal(err)
	}

	resolver := &memoryCommandResolver{sourceRoot: sourceRoot}
	got, err := resolver.Resolve()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	want := []string{
		"cargo", "run", "--quiet", "--bin", "memory", "--manifest-path", filepath.Join(sourceRoot, "Cargo.toml"), "--",
	}
	if len(got) != len(want) {
		t.Fatalf("unexpected arg count: got %v want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("arg %d mismatch: got %q want %q", i, got[i], want[i])
		}
	}
}

func mustWriteFile(t *testing.T, path string, content []byte) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, content, 0o644); err != nil {
		t.Fatal(err)
	}
}
