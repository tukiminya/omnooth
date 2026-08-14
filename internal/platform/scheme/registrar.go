package scheme

// Status は、ユーザー単位のハンドラーが存在し、OS上で有効かどうかを表す。
type Status struct {
	Installed bool
	Active    bool
	Detail    string
}

type Registrar interface {
	Install() error
	Status() (Status, error)
	Uninstall() error
}
