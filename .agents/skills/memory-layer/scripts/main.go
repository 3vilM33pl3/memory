package main

import (
	"errors"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

func main() {
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	if len(args) == 0 {
		printUsage(os.Stderr)
		return 2
	}

	resolver, err := newMemoryCommandResolver()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}

	var commandArgs []string
	switch args[0] {
	case "query-memory":
		commandArgs, err = buildQueryMemoryArgs(args[1:])
	case "resume-project":
		commandArgs, err = buildResumeProjectArgs(args[1:])
	case "checkpoint-project":
		commandArgs, err = buildCheckpointProjectArgs(args[1:])
	case "capture-task":
		commandArgs, err = buildCaptureTaskArgs(args[1:])
	case "curate-memory":
		commandArgs, err = buildCurateMemoryArgs(args[1:])
	case "start-plan-execution":
		commandArgs = append([]string{"checkpoint", "start-execution"}, args[1:]...)
	case "finish-plan-execution":
		commandArgs = append([]string{"checkpoint", "finish-execution"}, args[1:]...)
	case "remember-task", "remember-current-work":
		commandArgs = append([]string{"remember"}, args[1:]...)
	default:
		fmt.Fprintf(os.Stderr, "Unknown skill helper command: %s\n\n", args[0])
		printUsage(os.Stderr)
		return 2
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}

	cmdArgs, err := resolver.Resolve()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	cmdArgs = append(cmdArgs, commandArgs...)

	cmd := exec.Command(cmdArgs[0], cmdArgs[1:]...)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Env = os.Environ()
	if err := cmd.Run(); err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) {
			return exitErr.ExitCode()
		}
		fmt.Fprintln(os.Stderr, err)
		return 1
	}
	return 0
}

func printUsage(out *os.File) {
	fmt.Fprintln(out, "Usage: go run ./.agents/skills/memory-layer/scripts <command> [args]")
	fmt.Fprintln(out)
	fmt.Fprintln(out, "Commands:")
	fmt.Fprintln(out, "  query-memory <question> [project-slug]")
	fmt.Fprintln(out, "  resume-project [project-slug]")
	fmt.Fprintln(out, "  checkpoint-project [--project <slug>] [--note <text>]")
	fmt.Fprintln(out, "  capture-task <payload.json>")
	fmt.Fprintln(out, "  curate-memory [project-slug]")
	fmt.Fprintln(out, "  start-plan-execution <memory checkpoint start-execution args...>")
	fmt.Fprintln(out, "  finish-plan-execution <memory checkpoint finish-execution args...>")
	fmt.Fprintln(out, "  remember-task <memory remember args...>")
	fmt.Fprintln(out, "  remember-current-work <memory remember args...>")
}

type memoryCommandResolver struct {
	sourceRoot string
}

func newMemoryCommandResolver() (*memoryCommandResolver, error) {
	_, currentFile, _, ok := runtime.Caller(0)
	if !ok {
		return nil, errors.New("could not determine skill helper source path")
	}
	scriptDir := filepath.Dir(currentFile)
	sourceRoot := filepath.Clean(filepath.Join(scriptDir, "..", "..", "..", ".."))
	return &memoryCommandResolver{sourceRoot: sourceRoot}, nil
}

func (r *memoryCommandResolver) Resolve() ([]string, error) {
	if explicit := strings.TrimSpace(os.Getenv("MEMCTL_BIN")); explicit != "" {
		fields := strings.Fields(explicit)
		if len(fields) == 0 {
			return nil, errors.New("MEMCTL_BIN is set but empty")
		}
		return fields, nil
	}

	if _, err := exec.LookPath("memory"); err == nil {
		return []string{"memory"}, nil
	}

	manifestPath := filepath.Join(r.sourceRoot, "Cargo.toml")
	memCliManifestPath := filepath.Join(r.sourceRoot, "crates", "mem-cli", "Cargo.toml")
	if fileExists(manifestPath) && fileExists(memCliManifestPath) {
		return []string{
			"cargo", "run", "--quiet", "--bin", "memory", "--manifest-path", manifestPath, "--",
		}, nil
	}

	return nil, errors.New("Memory Layer CLI not found. Install `memory`, or set MEMCTL_BIN to an explicit command.")
}

func buildQueryMemoryArgs(args []string) ([]string, error) {
	if len(args) < 1 || strings.TrimSpace(args[0]) == "" {
		return nil, errors.New("Usage: query-memory \"<question>\" [project-slug]")
	}
	project := defaultProject()
	if len(args) > 1 {
		project = args[1]
	}
	if len(args) > 2 {
		return nil, errors.New("Usage: query-memory \"<question>\" [project-slug]")
	}
	return []string{"query", "--project", project, "--question", args[0]}, nil
}

func buildResumeProjectArgs(args []string) ([]string, error) {
	project := defaultProject()
	if len(args) > 1 {
		return nil, errors.New("Usage: resume-project [project-slug]")
	}
	if len(args) == 1 {
		project = args[0]
	}
	return []string{"resume", "--project", project}, nil
}

func buildCheckpointProjectArgs(args []string) ([]string, error) {
	fs := flag.NewFlagSet("checkpoint-project", flag.ContinueOnError)
	fs.SetOutput(os.Stderr)
	project := defaultProject()
	note := ""
	fs.StringVar(&project, "project", project, "")
	fs.StringVar(&note, "note", note, "")
	if err := fs.Parse(args); err != nil {
		return nil, err
	}
	if fs.NArg() != 0 {
		return nil, errors.New("Usage: checkpoint-project [--project <slug>] [--note <text>]")
	}
	commandArgs := []string{"checkpoint", "save", "--project", project}
	if strings.TrimSpace(note) != "" {
		commandArgs = append(commandArgs, "--note", note)
	}
	return commandArgs, nil
}

func buildCaptureTaskArgs(args []string) ([]string, error) {
	if len(args) != 1 {
		return nil, errors.New("Usage: capture-task <payload.json>")
	}
	if !fileExists(args[0]) {
		return nil, fmt.Errorf("Payload file not found: %s", args[0])
	}
	return []string{"capture", "task", "--file", args[0]}, nil
}

func buildCurateMemoryArgs(args []string) ([]string, error) {
	project := defaultProject()
	if len(args) > 1 {
		return nil, errors.New("Usage: curate-memory [project-slug]")
	}
	if len(args) == 1 {
		project = args[0]
	}
	return []string{"curate", "--project", project}, nil
}

func defaultProject() string {
	if project := strings.TrimSpace(os.Getenv("MEMORY_LAYER_PROJECT")); project != "" {
		return project
	}
	cwd, err := os.Getwd()
	if err != nil {
		return "unknown-project"
	}
	return filepath.Base(cwd)
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}
