# English day and month names

The installer sets the regional categories (`LC_TIME`, `LC_NUMERIC`, …) to
`nl_NL.UTF-8` from the machine's location while `LANG` is `en_GB.UTF-8`.
Anything that formats dates through the C library — the Noctalia bar clock,
`date`, `ls -l` — then prints Dutch day and month names. Keep the Dutch
paper/measurement/monetary conventions but format time in English:

```sh
sudo localectl set-locale LC_TIME=en_GB.UTF-8
```

`en_GB` keeps the 24-hour clock and day-first dates. Takes effect for new
sessions (re-login); already-running apps keep the old value.
