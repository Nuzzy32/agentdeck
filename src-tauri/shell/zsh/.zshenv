# agentdeck: integrasi zsh per peluncuran (D-031). Pane shell dijalankan dengan ZDOTDIR menunjuk ke folder ini, jadi
# zsh membaca berkas ini lebih dulu. ZDOTDIR langsung dihapus agar .zshenv, .zprofile, .zshrc, dan .zlogin pengguna
# dibaca dari tempat biasanya. Dotfile pengguna tidak diubah. Hook memancarkan OSC 133: A awal prompt, B akhir prompt,
# C perintah mulai, D;<exit> perintah selesai.
'builtin' 'unset' 'ZDOTDIR'
{
  [[ ! -r "$HOME/.zshenv" ]] || 'builtin' 'source' '--' "$HOME/.zshenv"
} always {
  if [[ -o interactive ]]; then
    'builtin' 'autoload' '-Uz' 'add-zsh-hook'
    'builtin' 'typeset' '-gi' _agentdeck_ran=0
    _agentdeck_precmd() {
      'builtin' 'local' s=$?
      (( _agentdeck_ran )) && 'builtin' 'print' '-n' "\e]133;D;$s\a"
      _agentdeck_ran=0
      'builtin' 'print' '-n' '\e]133;A\a'
      [[ $PS1 == *$'\e]133;B\a'* ]] || PS1+=$'%{\e]133;B\a%}'
    }
    _agentdeck_preexec() {
      _agentdeck_ran=1
      'builtin' 'print' '-n' '\e]133;C\a'
    }
    add-zsh-hook precmd _agentdeck_precmd
    add-zsh-hook preexec _agentdeck_preexec
  fi
}
