package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

type outputMode int

const (
	outputJSON outputMode = iota
	outputText
)

type helperInvocation struct {
	helperCommand  string
	project        string
	commandArgs    []string
	expectJSON     bool
	textPassthrough bool
}

type helperEnvelope struct {
	Ok            bool                 `json:"ok"`
	HelperCommand string               `json:"helper_command"`
	Project       string               `json:"project,omitempty"`
	Result        any                  `json:"result,omitempty"`
	Error         *helperError         `json:"error,omitempty"`
	Details       *helperErrorDetails  `json:"details,omitempty"`
}

type helperError struct {
	Kind     string `json:"kind"`
	Message  string `json:"message"`
	ExitCode *int   `json:"exit_code,omitempty"`
}

type helperErrorDetails struct {
	WrappedCommand []string `json:"wrapped_command,omitempty"`
	Stdout         string   `json:"stdout,omitempty"`
	Stderr         string   `json:"stderr,omitempty"`
}

func main() {
	os.Exit(run(os.Args[1:]))
}

func run(args []string) int {
	mode, args, parseErr := parseGlobalOptions(args)
	if parseErr != nil {
		return emitHelperFailure(mode, "", "", "usage_error", parseErr.Error(), 2, nil, "", "")
	}
	if len(args) == 0 {
		return emitHelperFailure(mode, "", "", "usage_error", usageText(), 2, nil, "", "")
	}

	invocation, err := buildInvocation(args, mode)
	if err != nil {
		return emitHelperFailure(mode, helperCommandName(args), "", "usage_error", err.Error(), 2, nil, "", "")
	}

	resolver, err := newMemoryCommandResolver()
	if err != nil {
		return emitHelperFailure(mode, invocation.helperCommand, invocation.project, "resolver_error", err.Error(), 1, nil, "", "")
	}

	cmdArgs, err := resolver.Resolve()
	if err != nil {
		return emitHelperFailure(mode, invocation.helperCommand, invocation.project, "resolver_error", err.Error(), 1, nil, "", "")
	}
	cmdArgs = append(cmdArgs, invocation.commandArgs...)

	cmd := exec.Command(cmdArgs[0], cmdArgs[1:]...)
	cmd.Stdin = os.Stdin
	cmd.Env = os.Environ()
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	runErr := cmd.Run()
	exitCode := 0
	if runErr != nil {
		var exitErr *exec.ExitError
		if errors.As(runErr, &exitErr) {
			exitCode = exitErr.ExitCode()
		} else {
			return emitHelperFailure(
				mode,
				invocation.helperCommand,
				invocation.project,
				"exec_error",
				runErr.Error(),
				1,
				cmdArgs,
				stdout.String(),
				stderr.String(),
			)
		}
	}

	if mode == outputText {
		return emitTextResult(invocation, stdout.String(), stderr.String(), exitCode)
	}

	if runErr != nil {
		message := strings.TrimSpace(stderr.String())
		if message == "" {
			message = strings.TrimSpace(stdout.String())
		}
		if message == "" {
			message = "wrapped command failed"
		}
		return emitHelperFailure(
			mode,
			invocation.helperCommand,
			invocation.project,
			"command_failed",
			message,
			exitCode,
			cmdArgs,
			stdout.String(),
			stderr.String(),
		)
	}

	var parsed any
	if invocation.expectJSON {
		if err := json.Unmarshal(stdout.Bytes(), &parsed); err != nil {
			return emitHelperFailure(
				mode,
				invocation.helperCommand,
				invocation.project,
				"invalid_output",
				fmt.Sprintf("wrapped command did not return valid JSON: %v", err),
				1,
				cmdArgs,
				stdout.String(),
				stderr.String(),
			)
		}
	} else {
		parsed = strings.TrimSpace(stdout.String())
	}

	return emitHelperSuccess(invocation.helperCommand, invocation.project, parsed)
}

func parseGlobalOptions(args []string) (outputMode, []string, error) {
	mode := outputJSON
	remaining := make([]string, 0, len(args))
	for _, arg := range args {
		switch arg {
		case "--text":
			mode = outputText
		default:
			remaining = append(remaining, arg)
		}
	}
	return mode, remaining, nil
}

func helperCommandName(args []string) string {
	if len(args) == 0 {
		return ""
	}
	return args[0]
}

func buildInvocation(args []string, mode outputMode) (helperInvocation, error) {
	if len(args) == 0 {
		return helperInvocation{}, errors.New(usageText())
	}

	switch args[0] {
	case "query-memory":
		return buildQueryMemoryInvocation(args[1:], mode)
	case "resume-project":
		return buildResumeProjectInvocation(args[1:], mode)
	case "checkpoint-project":
		return buildCheckpointProjectInvocation(args[1:], mode)
	case "capture-task":
		return buildCaptureTaskInvocation(args[1:])
	case "curate-memory":
		return buildCurateMemoryInvocation(args[1:])
	case "start-plan-execution":
		return helperInvocation{
			helperCommand:  "start-plan-execution",
			commandArgs:    append([]string{"checkpoint", "start-execution"}, args[1:]...),
			expectJSON:     true,
			textPassthrough: false,
		}, nil
	case "finish-plan-execution":
		commandArgs := append([]string{"checkpoint", "finish-execution"}, args[1:]...)
		if mode == outputJSON {
			commandArgs = append(commandArgs, "--json")
		}
		return helperInvocation{
			helperCommand:  "finish-plan-execution",
			commandArgs:    commandArgs,
			expectJSON:     mode == outputJSON,
			textPassthrough: mode == outputText,
		}, nil
	case "remember-task", "remember-current-work":
		return helperInvocation{
			helperCommand:  args[0],
			commandArgs:    append([]string{"remember"}, args[1:]...),
			expectJSON:     true,
			textPassthrough: false,
		}, nil
	default:
		return helperInvocation{}, fmt.Errorf("unknown skill helper command: %s\n\n%s", args[0], usageText())
	}
}

func usageText() string {
	return strings.TrimSpace(`Usage: go run ./.agents/skills/memory-layer/scripts/main.go [--text] <command> [args]

Commands:
  query-memory <question> [project-slug]
  resume-project [project-slug]
  checkpoint-project [--project <slug>] [--note <text>]
  capture-task <payload.json>
  curate-memory [project-slug]
  start-plan-execution <memory checkpoint start-execution args...>
  finish-plan-execution <memory checkpoint finish-execution args...>
  remember-task <memory remember args...>
  remember-current-work <memory remember args...>`)
}

func printUsage(out io.Writer) {
	fmt.Fprintln(out, usageText())
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

func buildQueryMemoryInvocation(args []string, mode outputMode) (helperInvocation, error) {
	if len(args) < 1 || strings.TrimSpace(args[0]) == "" {
		return helperInvocation{}, errors.New("Usage: query-memory \"<question>\" [project-slug]")
	}
	project := defaultProject()
	if len(args) > 1 {
		project = args[1]
	}
	if len(args) > 2 {
		return helperInvocation{}, errors.New("Usage: query-memory \"<question>\" [project-slug]")
	}
	commandArgs := []string{"query", "--project", project, "--question", args[0]}
	if mode == outputJSON {
		commandArgs = append(commandArgs, "--json")
	}
	return helperInvocation{
		helperCommand:  "query-memory",
		project:        project,
		commandArgs:    commandArgs,
		expectJSON:     mode == outputJSON,
		textPassthrough: mode == outputText,
	}, nil
}

func buildResumeProjectInvocation(args []string, mode outputMode) (helperInvocation, error) {
	project := defaultProject()
	if len(args) > 1 {
		return helperInvocation{}, errors.New("Usage: resume-project [project-slug]")
	}
	if len(args) == 1 {
		project = args[0]
	}
	commandArgs := []string{"resume", "--project", project}
	if mode == outputJSON {
		commandArgs = append(commandArgs, "--json")
	}
	return helperInvocation{
		helperCommand:  "resume-project",
		project:        project,
		commandArgs:    commandArgs,
		expectJSON:     mode == outputJSON,
		textPassthrough: mode == outputText,
	}, nil
}

func buildCheckpointProjectInvocation(args []string, mode outputMode) (helperInvocation, error) {
	fs := flag.NewFlagSet("checkpoint-project", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	project := defaultProject()
	note := ""
	fs.StringVar(&project, "project", project, "")
	fs.StringVar(&note, "note", note, "")
	if err := fs.Parse(args); err != nil {
		return helperInvocation{}, err
	}
	if fs.NArg() != 0 {
		return helperInvocation{}, errors.New("Usage: checkpoint-project [--project <slug>] [--note <text>]")
	}
	commandArgs := []string{"checkpoint", "save", "--project", project}
	if strings.TrimSpace(note) != "" {
		commandArgs = append(commandArgs, "--note", note)
	}
	if mode == outputJSON {
		commandArgs = append(commandArgs, "--json")
	}
	return helperInvocation{
		helperCommand:  "checkpoint-project",
		project:        project,
		commandArgs:    commandArgs,
		expectJSON:     mode == outputJSON,
		textPassthrough: mode == outputText,
	}, nil
}

func buildCaptureTaskInvocation(args []string) (helperInvocation, error) {
	if len(args) != 1 {
		return helperInvocation{}, errors.New("Usage: capture-task <payload.json>")
	}
	if !fileExists(args[0]) {
		return helperInvocation{}, fmt.Errorf("Payload file not found: %s", args[0])
	}
	return helperInvocation{
		helperCommand:  "capture-task",
		commandArgs:    []string{"capture", "task", "--file", args[0]},
		expectJSON:     true,
		textPassthrough: false,
	}, nil
}

func buildCurateMemoryInvocation(args []string) (helperInvocation, error) {
	project := defaultProject()
	if len(args) > 1 {
		return helperInvocation{}, errors.New("Usage: curate-memory [project-slug]")
	}
	if len(args) == 1 {
		project = args[0]
	}
	return helperInvocation{
		helperCommand:  "curate-memory",
		project:        project,
		commandArgs:    []string{"curate", "--project", project},
		expectJSON:     true,
		textPassthrough: false,
	}, nil
}

func emitTextResult(invocation helperInvocation, stdout, stderr string, exitCode int) int {
	if invocation.textPassthrough {
		if stdout != "" {
			fmt.Fprint(os.Stdout, stdout)
		}
		if stderr != "" {
			fmt.Fprint(os.Stderr, stderr)
		}
		return exitCode
	}

	var parsed any
	if err := json.Unmarshal([]byte(stdout), &parsed); err != nil {
		if stdout != "" {
			fmt.Fprint(os.Stdout, stdout)
		}
		if stderr != "" {
			fmt.Fprint(os.Stderr, stderr)
		}
		if exitCode != 0 {
			return exitCode
		}
		if stdout == "" {
			fmt.Fprintln(os.Stderr, "wrapped command returned no output")
			return 1
		}
		return 0
	}

	fmt.Fprintln(os.Stdout, renderTextSummary(invocation, parsed))
	if stderr != "" {
		fmt.Fprint(os.Stderr, stderr)
	}
	return exitCode
}

func emitHelperSuccess(helperCommand, project string, result any) int {
	envelope := helperEnvelope{
		Ok:            true,
		HelperCommand: helperCommand,
		Project:       project,
		Result:        result,
	}
	body, _ := json.MarshalIndent(envelope, "", "  ")
	fmt.Println(string(body))
	return 0
}

func emitHelperFailure(mode outputMode, helperCommand, project, kind, message string, exitCode int, wrapped []string, stdout, stderr string) int {
	if mode == outputText {
		if message != "" {
			fmt.Fprintln(os.Stderr, message)
		}
		if stderr != "" && !strings.Contains(stderr, message) {
			fmt.Fprint(os.Stderr, stderr)
		}
		return exitCode
	}

	code := exitCode
	envelope := helperEnvelope{
		Ok:            false,
		HelperCommand: helperCommand,
		Project:       project,
		Error: &helperError{
			Kind:     kind,
			Message:  message,
			ExitCode: &code,
		},
	}
	if len(wrapped) > 0 || strings.TrimSpace(stdout) != "" || strings.TrimSpace(stderr) != "" {
		envelope.Details = &helperErrorDetails{
			WrappedCommand: wrapped,
			Stdout:         strings.TrimSpace(stdout),
			Stderr:         strings.TrimSpace(stderr),
		}
	}
	body, _ := json.MarshalIndent(envelope, "", "  ")
	fmt.Println(string(body))
	return exitCode
}

func renderTextSummary(invocation helperInvocation, result any) string {
	switch invocation.helperCommand {
	case "start-plan-execution":
		return renderStartPlanExecutionText(result)
	case "remember-task", "remember-current-work":
		return renderRememberText(result)
	case "capture-task":
		return renderCaptureText(result)
	case "curate-memory":
		return renderCurateText(result)
	default:
		pretty, err := json.MarshalIndent(result, "", "  ")
		if err != nil {
			return fmt.Sprintf("%v", result)
		}
		return string(pretty)
	}
}

func renderStartPlanExecutionText(result any) string {
	checkpointPath := nestedString(result, "checkpoint", "path")
	planTitle := nestedString(result, "plan", "title")
	threadKey := nestedString(result, "plan", "thread_key")
	totalItems := nestedNumber(result, "plan", "total_items")
	rawCaptureID := nestedString(result, "capture", "raw_capture_id")
	curateRunID := nestedString(result, "curate", "run_id")
	dryRun := nestedBool(result, "dry_run")

	lines := []string{"Plan execution started."}
	if checkpointPath != "" {
		lines = append(lines, fmt.Sprintf("Checkpoint: %s", checkpointPath))
	}
	if planTitle != "" || threadKey != "" || totalItems != "" {
		lines = append(lines, fmt.Sprintf("Plan: %s (%s) %s items", fallback(planTitle, "n/a"), fallback(threadKey, "n/a"), fallback(totalItems, "0")))
	}
	if rawCaptureID != "" {
		lines = append(lines, fmt.Sprintf("Capture: %s", rawCaptureID))
	}
	if curateRunID != "" {
		lines = append(lines, fmt.Sprintf("Curate run: %s", curateRunID))
	}
	if dryRun == "true" {
		lines = append(lines, "Dry run: true")
	}
	return strings.Join(lines, "\n")
}

func renderRememberText(result any) string {
	rawCaptureID := nestedString(result, "capture", "raw_capture_id")
	curateRunID := nestedString(result, "curate", "run_id")
	dryRun := nestedBool(result, "dry_run")
	lines := []string{"Remembered task context."}
	if rawCaptureID != "" {
		lines = append(lines, fmt.Sprintf("Capture: %s", rawCaptureID))
	}
	if curateRunID != "" {
		lines = append(lines, fmt.Sprintf("Curate run: %s", curateRunID))
	}
	if dryRun == "true" {
		lines = append(lines, "Dry run: true")
	}
	return strings.Join(lines, "\n")
}

func renderCaptureText(result any) string {
	rawCaptureID := nestedString(result, "raw_capture_id")
	taskID := nestedString(result, "task_id")
	dryRun := nestedBool(result, "dry_run")
	lines := []string{"Captured task evidence."}
	if rawCaptureID != "" {
		lines = append(lines, fmt.Sprintf("Raw capture: %s", rawCaptureID))
	}
	if taskID != "" {
		lines = append(lines, fmt.Sprintf("Task: %s", taskID))
	}
	if dryRun == "true" {
		lines = append(lines, "Dry run: true")
	}
	return strings.Join(lines, "\n")
}

func renderCurateText(result any) string {
	runID := nestedString(result, "run_id")
	inputCount := nestedNumber(result, "input_count")
	outputCount := nestedNumber(result, "output_count")
	replacedCount := nestedNumber(result, "replaced_count")
	proposalCount := nestedNumber(result, "proposal_count")
	dryRun := nestedBool(result, "dry_run")
	lines := []string{"Curated raw captures into memory."}
	if runID != "" {
		lines = append(lines, fmt.Sprintf("Run: %s", runID))
	}
	if inputCount != "" || outputCount != "" {
		lines = append(lines, fmt.Sprintf("Input: %s   Output: %s", fallback(inputCount, "0"), fallback(outputCount, "0")))
	}
	if replacedCount != "" || proposalCount != "" {
		lines = append(lines, fmt.Sprintf("Replaced: %s   Proposals: %s", fallback(replacedCount, "0"), fallback(proposalCount, "0")))
	}
	if dryRun == "true" {
		lines = append(lines, "Dry run: true")
	}
	return strings.Join(lines, "\n")
}

func nestedString(data any, keys ...string) string {
	current := data
	for _, key := range keys {
		m, ok := current.(map[string]any)
		if !ok {
			return ""
		}
		current, ok = m[key]
		if !ok {
			return ""
		}
	}
	value, ok := current.(string)
	if !ok {
		return ""
	}
	return value
}

func nestedNumber(data any, keys ...string) string {
	current := data
	for _, key := range keys {
		m, ok := current.(map[string]any)
		if !ok {
			return ""
		}
		current, ok = m[key]
		if !ok {
			return ""
		}
	}
	switch value := current.(type) {
	case float64:
		if value == float64(int64(value)) {
			return fmt.Sprintf("%d", int64(value))
		}
		return fmt.Sprintf("%.2f", value)
	case int:
		return fmt.Sprintf("%d", value)
	case int64:
		return fmt.Sprintf("%d", value)
	default:
		return ""
	}
}

func nestedBool(data any, keys ...string) string {
	current := data
	for _, key := range keys {
		m, ok := current.(map[string]any)
		if !ok {
			return ""
		}
		current, ok = m[key]
		if !ok {
			return ""
		}
	}
	value, ok := current.(bool)
	if !ok {
		return ""
	}
	if value {
		return "true"
	}
	return "false"
}

func fallback(value, defaultValue string) string {
	if strings.TrimSpace(value) == "" {
		return defaultValue
	}
	return value
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
