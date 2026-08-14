package cmd

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/spf13/cobra"
	"github.com/tukiminya/omnooth/internal/importer"
	"github.com/tukiminya/omnooth/internal/platform"
	"github.com/tukiminya/omnooth/internal/platform/scheme"
)

func Execute() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	root := NewRootCommand()
	if err := root.ExecuteContext(ctx); err != nil {
		fmt.Fprintln(os.Stderr, "Error:", err)
		os.Exit(1)
	}
}

func NewRootCommand() *cobra.Command {
	root := &cobra.Command{
		Use:           "omnooth",
		Short:         "Download and organize items from your BOOTH library",
		SilenceErrors: true,
		SilenceUsage:  true,
	}
	root.AddCommand(newImportCommand(), newSchemeCommand())
	return root
}

func newImportCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "import <booth-library-manager URL>",
		Short: "Import one BOOTH downloadable",
		Args:  cobra.ExactArgs(1),
		RunE: func(command *cobra.Command, args []string) error {
			result, err := importURL(command.Context(), args[0])
			if err != nil {
				return err
			}
			fmt.Fprintln(command.OutOrStdout(), result.Destination)
			return nil
		},
	}
}

func importURL(ctx context.Context, rawURL string) (importer.ImportResult, error) {
	request, err := importer.ParseImportURI(rawURL)
	if err != nil {
		return importer.ImportResult{}, err
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return importer.ImportResult{}, fmt.Errorf("find home directory: %w", err)
	}
	client := platform.NewHTTPClient()
	service := importer.Importer{
		Catalog:    platform.NewBoothCatalog(client),
		Downloader: platform.HTTPDownloader{Client: client},
		Extractor:  platform.ArchiveMaterializer{},
		Store:      platform.NewLocalStore(filepath.Join(home, "omnooth")),
	}
	return service.Import(ctx, request)
}

func newSchemeCommand() *cobra.Command {
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
				_ = scheme.RecordHandlerDiagnostic("received", "")
				result, err := importURL(command.Context(), args[0])
				if err != nil {
					_ = scheme.RecordHandlerDiagnostic("failed", handlerFailureCode(err))
					return err
				}
				_ = scheme.RecordHandlerDiagnostic("succeeded", result.Destination)
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
