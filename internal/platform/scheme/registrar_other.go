//go:build !windows && !linux && !darwin

package scheme

import "fmt"

func NewRegistrar() (Registrar, error) {
	return nil, fmt.Errorf("URL scheme registration is not supported on this operating system")
}
