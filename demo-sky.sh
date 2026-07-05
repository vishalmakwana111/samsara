#!/usr/bin/env bash
# Live preview of samsara's constellation: stars twinkle, and every few seconds a
# comet streaks from a burned-out star to the one reborn as active. Ctrl-C to stop.
esc() { printf '\033[38;2;%s;%s;%sm' "$1" "$2" "$3"; }
R='\033[0m'; B='\033[1m'
GOLD='240 196 110'; SAFF='245 158 66'; VIO='150 120 232'; ASH='110 114 130'
FAINT='72 76 92'; GREEN='124 206 140'; CYAN='120 200 220'; EMBER='232 120 92'

# star seats on a 5x27 grid: "row col label state"  (state: active|ready|cool)
seats=( "0 4 work active" "1 14 personal ready" "0 21 backup ready" "2 9 spare cool" )
dust=( "0 11" "1 22" "2 2" "2 17" "3 7" "4 3" "4 20" )

draw_sky() { # $1 = twinkle seed
  local seed=$1
  for r in 0 1 2 3 4; do
    local line=""
    for c in $(seq 0 26); do
      local ch=" "
      for d in "${dust[@]}"; do set -- $d; [ "$1" = "$r" ] && [ "$2" = "$c" ] && ch="$(esc $FAINT)·$R"; done
      for s in "${seats[@]}"; do
        set -- $s
        if [ "$1" = "$r" ] && [ "$2" = "$c" ]; then
          local tw=$(( (seed + r*7 + c) % 5 ))
          case "$4" in
            active) [ $tw -lt 2 ] && ch="$B$(esc $SAFF)✧$R" || ch="$B$(esc $GOLD)✦$R" ;;
            ready)  [ $tw -eq 0 ] && ch="$(esc $FAINT)✦$R" || ch="$(esc $GREEN)✦$R" ;;
            cool)   ch="$(esc $CYAN)✦$R" ;;
          esac
        fi
      done
      line="$line$ch"
    done
    printf '   %b\n' "$line"
  done
}

comet() { # streak a comet across the middle row
  for i in $(seq 0 22); do
    printf '\033[2A'                                   # up into the sky block
    printf '\r\033[2K   %*s%b%b%b\n' "$i" "" "$(esc $FAINT)·∙" "$(esc $SAFF)━" "$B$(esc $GOLD)✦$R"
    printf '\033[1B'
    sleep 0.02
  done
}

cleanup() { printf '\033[?25h\n'; exit 0; }
trap cleanup INT TERM
printf '\033[?25l'
printf '\n   %b✦%b %bthe night sky of your keys%b\n\n' "$B$(esc $GOLD)" "$R" "$(esc $ASH)" "$R"
draw_sky 0
frame=0
while true; do
  printf '\033[5A'          # jump back up over the 5 sky rows
  draw_sky "$frame"
  sleep 0.22
  frame=$((frame + 1))
  if [ $((frame % 18)) -eq 0 ]; then
    printf '\n   %b✶ spare%b burned out — the wheel turns…\n' "$(esc $EMBER)" "$R"
    sleep 0.4
    printf '\033[1A\033[2K'
  fi
done
