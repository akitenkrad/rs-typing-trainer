Output "/tmp/rs-typing-trainer-menu.gif"
Set Shell "bash"
Set FontSize 14
Set Width 1000
Set Height 560
Set Padding 10
Set TypingSpeed 0
Hide
Type "tmpdir=$(mktemp -d /tmp/rs-typing-trainer-menu.XXXXXX) && printf '[storage]\ndb_path = \047%s/sessions.db\047\n[display]\ngraphics = \047off\047\n[session]\nresult_input_delay_ms = 0\n' $tmpdir > $tmpdir/config.toml && target/debug/rs-typing-trainer --config $tmpdir/config.toml --lang en --graphics off play"
Enter
Sleep 2s
Show
Sleep 1s
Type "m"
Sleep 1s
Enter
Sleep 500ms
Enter
Sleep 4s
Sleep 121s
Sleep 2s
