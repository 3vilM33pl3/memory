package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func main() {
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	if len(args) == 0 {
		printUsage()
		return 2
	}

	command, err := resolveCommand()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 1
	}

	var forward []string
	switch args[0] {
	case "query-memory":
		if len(args) < 2 {
			fmt.Fprintln(os.Stderr, "Usage: query-memory \"<question>\" [project-slug]")
			return 2
		}
		project := defaultProject()
		if len(args) > 2 {
			project = args[2]
		}
		forward = []string{"query", "--project", project, "--question", args[1]}
	case "capture-task":
		if len(args) != 2 {
			fmt.Fprintln(os.Stderr, "Usage: capture-task <payload.json>")
			return 2
		}
		if _, err := os.Stat(args[1]); err != nil {
			fmt.Fprintf(os.Stderr, "Payload file not found: %s\n", args[1])
			return 2
		}
		forward = []string{"capture", "task", "--file", args[1]}
	case "curate-memory":
		project := defaultProject()
		if len(args) > 1 {
			project = args[1]
		}
		forward = []string{"curate", "--project", project}
	default:
		fmt.Fprintf(os.Stderr, "Unknown command: %s\n", args[0])
		printUsage()
		return 2
	}

	command = append(command, forward...)
	cmd := exec.Command(command[0], command[1:]...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
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

func printUsage() {
	fmt.Fprintln(os.Stderr, "Usage: go run ./.agents/skills/memory-layer/scripts/main.go <query-memory|capture-task|curate-memory> [args]")
}

func resolveCommand() ([]string, error) {
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
	return nil, errors.New("Memory Layer CLI not found. Install `memory`, or set MEMCTL_BIN to an explicit command.")
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
