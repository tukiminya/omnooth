package cmd

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"text/tabwriter"
	"time"
	"unicode"

	"github.com/spf13/cobra"
	"github.com/tukiminya/omnooth/internal/importer"
	"github.com/tukiminya/omnooth/internal/outdated"
	"github.com/tukiminya/omnooth/internal/platform"
	"github.com/tukiminya/omnooth/internal/platform/scheme"
	"github.com/tukiminya/omnooth/internal/runlog"
)

func Execute() {
	os.Exit(execute(os.Args[1:], os.Stdout, os.Stderr))
}

func execute(args []string, stdout, stderr io.Writer) (exitCode int) {
	started := time.Now()
	logger := slog.New(slog.DiscardHandler)
	var session *runlog.Session

	home, homeErr := os.UserHomeDir()
	if homeErr != nil {
		fmt.Fprintf(stderr, "Warning: logging unavailable: find home directory: %v\n", homeErr)
	} else {
		var logErr error
		session, logErr = runlog.Open(home, started)
		if logErr != nil {
			fmt.Fprintf(stderr, "Warning: logging unavailable: %v\n", logErr)
		} else {
			logger = session.Logger
			defer func() {
				if err := session.Close(); err != nil {
					fmt.Fprintf(stderr, "Warning: close log %s: %v\n", session.Path, err)
				}
			}()
		}
	}

	defer func() {
		if panicValue := recover(); panicValue != nil {
			logger.Error("process panicked", append([]any{"event", "process_panicked", "duration", time.Since(started)}, runlog.PanicAttrs(panicValue)...)...)
			fmt.Fprintln(stderr, "Error: unexpected internal failure")
			if session != nil {
				fmt.Fprintln(stderr, "Log:", session.Path)
			}
			exitCode = 1
		}
	}()

	commandName := commandLabel(args)
	logger.Info("process started", "event", "process_started", "command", commandName)
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()

	root := newRootCommand(logger)
	root.SetArgs(args)
	root.SetOut(stdout)
	root.SetErr(stderr)
	if err := root.ExecuteContext(ctx); err != nil {
		logger.Error("command failed", append([]any{"event", "command_failed", "command", commandName, "duration", time.Since(started)}, runlog.ErrorAttrs(err)...)...)
		fmt.Fprintln(stderr, "Error:", runlog.Sanitize(err.Error()))
		if session != nil {
			fmt.Fprintln(stderr, "Log:", session.Path)
		}
		return 1
	}

	logger.Info("command completed", "event", "command_completed", "command", commandName, "duration", time.Since(started))
	return 0
}

func commandLabel(args []string) string {
	if len(args) == 0 {
		return "omnooth"
	}
	switch args[0] {
	case "import":
		return "omnooth import"
	case "outdated":
		return "omnooth outdated"
	case "scheme":
		if len(args) > 1 {
			switch args[1] {
			case "install", "status", "uninstall", "handle":
				return "omnooth scheme " + args[1]
			}
		}
		return "omnooth scheme"
	default:
		return "omnooth"
	}
}

func NewRootCommand() *cobra.Command {
	return newRootCommand(slog.New(slog.DiscardHandler))
}

func newRootCommand(logger *slog.Logger) *cobra.Command {
	root := &cobra.Command{
		Use:           "omnooth",
		Short:         "Download and organize items from your BOOTH library",
		SilenceErrors: true,
		SilenceUsage:  true,
	}
	root.AddCommand(newImportCommand(logger), newOutdatedCommand(), newSchemeCommand(logger))
	return root
}

func newImportCommand(logger *slog.Logger) *cobra.Command {
	return &cobra.Command{
		Use:   "import <booth-library-manager URL>",
		Short: "Import one BOOTH downloadable",
		Args:  cobra.ExactArgs(1),
		RunE: func(command *cobra.Command, args []string) error {
			result, err := importURL(command.Context(), args[0], logger)
			if err != nil {
				return err
			}
			fmt.Fprintln(command.OutOrStdout(), result.Destination)
			return nil
		},
	}
}

func newOutdatedCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "outdated",
		Short: "Show imported BOOTH items whose downloadables changed",
		Args:  cobra.NoArgs,
		RunE: func(command *cobra.Command, _ []string) error {
			home, err := os.UserHomeDir()
			if err != nil {
				return fmt.Errorf("find home directory: %w", err)
			}
			client := platform.NewHTTPClient()
			result, err := (outdated.Service{
				Catalog: platform.NewBoothCatalog(client),
				Root:    filepath.Join(home, "omnooth"),
			}).Check(command.Context())
			if err != nil {
				return err
			}
			if err := writeOutdated(command.OutOrStdout(), result); err != nil {
				return err
			}
			if result.HadErrors {
				return errors.New("some installed items could not be checked")
			}
			return nil
		},
	}
}

func writeOutdated(output io.Writer, result outdated.Result) error {
	if len(result.Reports) == 0 {
		_, err := fmt.Fprintln(output, "All installed items are up to date.")
		return err
	}
	writer := tabwriter.NewWriter(output, 0, 4, 2, ' ', 0)
	if _, err := fmt.Fprintln(writer, "STATUS\tITEM\tVARIATION\tLOCAL DOWNLOADABLES\tDETAIL"); err != nil {
		return err
	}
	for _, report := range result.Reports {
		item := strconv.FormatInt(report.ItemID, 10)
		if report.ItemName != "" {
			item += " " + displayText(report.ItemName)
		}
		variation := "-"
		if report.VariationID > 0 {
			variation = strconv.FormatInt(report.VariationID, 10)
			if report.VariationName != "" {
				variation += " " + displayText(report.VariationName)
			}
		}
		detail := report.Reason
		if len(report.Changes) > 0 {
			detail = strings.Join(report.Changes, "; ")
		}
		if _, err := fmt.Fprintf(writer, "%s\t%s\t%s\t%s\t%s\n",
			report.Status, item, variation, displayText(strings.Join(report.LocalDownloadables, ", ")), displayText(detail)); err != nil {
			return err
		}
	}
	return writer.Flush()
}

func displayText(value string) string {
	return strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, value)
}

func importURL(ctx context.Context, rawURL string, logger *slog.Logger) (importer.ImportResult, error) {
	stageStarted := time.Now()
	logger.Info("stage started", "event", "stage_started", "stage", "parse_import_uri")
	request, err := importer.ParseImportURI(rawURL)
	if err != nil {
		logger.Error("stage failed", append([]any{"event", "stage_failed", "stage", "parse_import_uri", "duration", time.Since(stageStarted)}, runlog.ErrorAttrs(err)...)...)
		return importer.ImportResult{}, err
	}
	logger.Info("stage completed", "event", "stage_completed", "stage", "parse_import_uri", "duration", time.Since(stageStarted), "item_id", request.ItemID, "variation_id", request.VariationID)

	home, err := os.UserHomeDir()
	if err != nil {
		err = fmt.Errorf("find home directory: %w", err)
		logger.Error("stage failed", append([]any{"event", "stage_failed", "stage", "resolve_library"}, runlog.ErrorAttrs(err)...)...)
		return importer.ImportResult{}, err
	}
	client := platform.NewHTTPClient()
	service := importer.Importer{
		Catalog:    platform.NewBoothCatalog(client),
		Downloader: platform.HTTPDownloader{Client: client},
		Extractor:  platform.ArchiveMaterializer{},
		Store:      platform.NewLocalStore(filepath.Join(home, "omnooth")),
		Logger:     logger,
	}
	return service.Import(ctx, request)
}

func newSchemeCommand(logger *slog.Logger) *cobra.Command {
	schemeCommand := &cobra.Command{
		Use:   "scheme",
		Short: "Manage the booth-library-manager URL scheme",
	}
	schemeCommand.AddCommand(
		&cobra.Command{
			Use:   "install",
			Short: "Install and register the per-user URL handler",
			Args:  cobra.NoArgs,
			RunE: func(command *cobra.Command, _ []string) error {
				registrar, err := scheme.NewRegistrar()
				if err != nil {
					return err
				}
				if err := registrar.Install(); err != nil {
					return err
				}
				fmt.Fprintln(command.OutOrStdout(), "booth-library-manager URL scheme installed")
				return nil
			},
		},
		&cobra.Command{
			Use:   "status",
			Short: "Show URL handler registration status",
			Args:  cobra.NoArgs,
			RunE: func(command *cobra.Command, _ []string) error {
				registrar, err := scheme.NewRegistrar()
				if err != nil {
					return err
				}
				status, err := registrar.Status()
				if err != nil {
					return err
				}
				fmt.Fprintf(command.OutOrStdout(), "installed: %t\nactive: %t\ndetail: %s\n", status.Installed, status.Active, status.Detail)
				diagnostic, found, err := scheme.ReadHandlerDiagnostic()
				if err != nil {
					fmt.Fprintf(command.OutOrStdout(), "last_handler_state: unavailable (%v)\n", err)
					return nil
				}
				if found {
					fmt.Fprintf(command.OutOrStdout(), "last_handler_event: %s\nlast_handler_state: %s\n", diagnostic.Timestamp.Format("2006-01-02T15:04:05Z07:00"), diagnostic.State)
					if diagnostic.Detail != "" {
						fmt.Fprintf(command.OutOrStdout(), "last_handler_detail: %s\n", diagnostic.Detail)
					}
				}
				return nil
			},
		},
		&cobra.Command{
			Use:   "uninstall",
			Short: "Unregister the URL handler without removing library data",
			Args:  cobra.NoArgs,
			RunE: func(command *cobra.Command, _ []string) error {
				registrar, err := scheme.NewRegistrar()
				if err != nil {
					return err
				}
				if err := registrar.Uninstall(); err != nil {
					return err
				}
				fmt.Fprintln(command.OutOrStdout(), "booth-library-manager URL scheme uninstalled")
				return nil
			},
		},
		&cobra.Command{
			Use:    "handle <booth-library-manager URL>",
			Hidden: true,
			Args:   cobra.ExactArgs(1),
			RunE: func(command *cobra.Command, args []string) error {
				// URLには署名付きダウンロード情報が含まれるため、URL自体は保存しない。
				if err := scheme.RecordHandlerDiagnostic("received", ""); err != nil {
					logger.Warn("handler diagnostic failed", append([]any{"event", "handler_diagnostic_failed", "state", "received"}, runlog.ErrorAttrs(err)...)...)
				}
				result, err := importURL(command.Context(), args[0], logger)
				if err != nil {
					if diagnosticErr := scheme.RecordHandlerDiagnostic("failed", handlerFailureCode(err)); diagnosticErr != nil {
						logger.Warn("handler diagnostic failed", append([]any{"event", "handler_diagnostic_failed", "state", "failed"}, runlog.ErrorAttrs(diagnosticErr)...)...)
					}
					return err
				}
				if err := scheme.RecordHandlerDiagnostic("succeeded", result.Destination); err != nil {
					logger.Warn("handler diagnostic failed", append([]any{"event", "handler_diagnostic_failed", "state", "succeeded"}, runlog.ErrorAttrs(err)...)...)
				}
				return nil
			},
		},
	)
	return schemeCommand
}

func handlerFailureCode(err error) string {
	switch {
	case errors.Is(err, importer.ErrInvalidImportURI):
		return "invalid_import_uri"
	case errors.Is(err, importer.ErrUntrustedDownloadURL):
		return "untrusted_download_url"
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return "canceled"
	default:
		return "import_failed"
	}
}
