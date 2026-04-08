package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestBuildQueryMemoryArgsUsesDefaultProject(t *testing.T) {
	oldEnv := os.Getenv("MEMORY_LAYER_PROJECT")
	t.Cleanup(func() { _ = os.Setenv("MEMORY_LAYER_PROJECT", oldEnv) })
	if err := os.Setenv("MEMORY_LAYER_PROJECT", "mem-project"); err != nil {
		t.Fatal(err)
	}

	got, err := buildQueryMemoryArgs([]string{"How does this repo work?"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	want := []string{"query", "--project", "mem-project", "--question", "How does this repo work?"}
	if len(got) != len(want) {
		t.Fatalf("unexpected arg count: got %v want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("arg %d mismatch: got %q want %q", i, got[i], want[i])
		}
	}
}

func TestBuildCheckpointProjectArgs(t *testing.T) {
	got, err := buildCheckpointProjectArgs([]string{"--project", "memory", "--note", "Plan approved"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	want := []string{"checkpoint", "save", "--project", "memory", "--note", "Plan approved"}
	if len(got) != len(want) {
		t.Fatalf("unexpected arg count: got %v want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("arg %d mismatch: got %q want %q", i, got[i], want[i])
		}
	}
}

func TestBuildCaptureTaskArgsRejectsMissingFile(t *testing.T) {
	_, err := buildCaptureTaskArgs([]string{"/tmp/does-not-exist.json"})
	if err == nil {
		t.Fatal("expected missing file error")
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
