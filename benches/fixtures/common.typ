#let int-input(name, default) = {
  let value = sys.inputs.at(name, default: str(default))
  int(value)
}
