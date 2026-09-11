# Source this file from zsh or bash to make `fwt cd <branch>` change the
# current shell's directory. Every other command is passed to the binary.
fwt() {
  if [ "${1-}" = "cd" ] && [ "$#" -eq 2 ] && [ "${2#-}" = "$2" ]; then
    shift
    local fwt_destination
    fwt_destination="$(command git-fwt resolve "$@")" || return $?
    builtin cd -- "$fwt_destination" || return $?
  else
    command git-fwt "$@"
  fi
}
