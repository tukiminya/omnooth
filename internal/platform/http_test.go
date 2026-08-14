package platform

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"testing"
)

func TestBoothCatalog(t *testing.T) {
	doer := doerFunc(func(request *http.Request) (*http.Response, error) {
		if request.Header.Get("Accept") != "application/json" {
			t.Error("missing Accept header")
		}
		return &http.Response{
			StatusCode: http.StatusOK,
			Body:       io.NopCloser(strings.NewReader(`{"id":8657397,"name":"【言葉をおぼえる】ムチォ","shop":{"id":838775,"name":"IWANUGA"}}`)),
		}, nil
	})
	catalog := &BoothCatalog{Client: doer, BaseURL: "https://api.booth.pm/vroid/items/"}
	metadata, err := catalog.GetItem(context.Background(), 8657397)
	if err != nil {
		t.Fatal(err)
	}
	if metadata.ShopID != 838775 || metadata.ShopName != "IWANUGA" || metadata.ItemID != 8657397 {
		t.Fatalf("metadata = %+v", metadata)
	}
}

func TestBoothCatalogFailures(t *testing.T) {
	responses := []struct {
		name   string
		status int
		body   string
	}{
		{"non-200", http.StatusNotFound, `{}`},
		{"invalid JSON", http.StatusOK, `{`},
		{"wrong ID", http.StatusOK, `{"id":1,"name":"Item","shop":{"id":2,"name":"Shop"}}`},
		{"missing fields", http.StatusOK, `{"id":8657397,"name":"","shop":{"id":2,"name":"Shop"}}`},
	}
	for _, test := range responses {
		t.Run(test.name, func(t *testing.T) {
			doer := doerFunc(func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: test.status, Body: io.NopCloser(strings.NewReader(test.body))}, nil
			})
			catalog := &BoothCatalog{Client: doer, BaseURL: "https://api.booth.pm/vroid/items/"}
			if _, err := catalog.GetItem(context.Background(), 8657397); err == nil {
				t.Fatal("expected an error")
			}
		})
	}
}

type doerFunc func(*http.Request) (*http.Response, error)

func (f doerFunc) Do(request *http.Request) (*http.Response, error) { return f(request) }

func TestHTTPDownloader(t *testing.T) {
	doer := doerFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(strings.NewReader("asset"))}, nil
	})
	source, _ := url.Parse("https://download.booth.pm/signed")
	destination := t.TempDir() + "/download"
	if err := (HTTPDownloader{Client: doer}).Download(context.Background(), source, destination); err != nil {
		t.Fatal(err)
	}
	content, _ := os.ReadFile(destination)
	if string(content) != "asset" {
		t.Fatalf("content = %q", content)
	}
}

func TestHTTPDownloaderDoesNotLeakSignedURLAndRemovesPartialFile(t *testing.T) {
	source, _ := url.Parse("https://booth.pm/file?signature=very-secret")
	destination := t.TempDir() + "/download"
	doer := doerFunc(func(*http.Request) (*http.Response, error) {
		return nil, errors.New("request https://booth.pm/file?signature=very-secret failed")
	})
	err := (HTTPDownloader{Client: doer}).Download(context.Background(), source, destination)
	if err == nil || strings.Contains(err.Error(), "very-secret") {
		t.Fatalf("unsafe error = %v", err)
	}
	if _, statErr := os.Stat(destination); !os.IsNotExist(statErr) {
		t.Fatalf("partial file remains: %v", statErr)
	}
}

func TestHTTPDownloaderRemovesInterruptedPartialFile(t *testing.T) {
	source, _ := url.Parse("https://booth.pm/file")
	destination := t.TempDir() + "/download"
	doer := doerFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: http.StatusOK, Body: io.NopCloser(&failingReader{})}, nil
	})
	if err := (HTTPDownloader{Client: doer}).Download(context.Background(), source, destination); err == nil {
		t.Fatal("expected an interrupted body error")
	}
	if _, err := os.Stat(destination); !os.IsNotExist(err) {
		t.Fatalf("partial file remains: %v", err)
	}
}

func TestHTTPClientRejectsExternalRedirect(t *testing.T) {
	client := NewHTTPClient()
	request := &http.Request{URL: mustURL(t, "https://evil.example/file")}
	if err := client.CheckRedirect(request, []*http.Request{{URL: mustURL(t, "https://booth.pm/file")}}); err == nil {
		t.Fatal("expected redirect rejection")
	}
}

func mustURL(t *testing.T, raw string) *url.URL {
	t.Helper()
	u, err := url.Parse(raw)
	if err != nil {
		t.Fatal(err)
	}
	return u
}

type failingReader struct {
	read bool
}

func (r *failingReader) Read(buffer []byte) (int, error) {
	if !r.read {
		r.read = true
		return copy(buffer, "partial"), nil
	}
	return 0, errors.New("connection interrupted")
}
