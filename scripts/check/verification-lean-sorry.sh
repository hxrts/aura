#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
target_dir="${1:-$repo_root/verification/lean/Aura}"

perl -ne '
BEGIN {
  $block_depth = 0;
}

sub scrub_comments {
  my ($line) = @_;
  my $out = q{};
  my $i = 0;
  my $len = length($line);
  my $in_string = 0;

  while ($i < $len) {
    my $ch = substr($line, $i, 1);
    my $next = $i + 1 < $len ? substr($line, $i + 1, 1) : q{};

    if ($block_depth > 0) {
      if ($ch eq "/" && $next eq "-") {
        $block_depth++;
        $i += 2;
        next;
      }
      if ($ch eq "-" && $next eq "/") {
        $block_depth--;
        $i += 2;
        next;
      }
      $i++;
      next;
    }

    if (!$in_string && $ch eq "/" && $next eq "-") {
      $block_depth++;
      $i += 2;
      next;
    }

    if (!$in_string && $ch eq "-" && $next eq "-") {
      last;
    }

    if ($ch eq "\"") {
      my $escaped = $i > 0 && substr($line, $i - 1, 1) eq "\\";
      if (!$escaped) {
        $in_string = !$in_string;
      }
    }

    $out .= $ch;
    $i++;
  }

  return $out;
}

my $clean = scrub_comments($_);
if ($clean =~ /\bsorry\b/) {
  print "$ARGV:$.:$_";
  $found = 1;
}

END {
  exit($found ? 0 : 1);
}
' $(find "$target_dir" -name '*.lean' -type f | sort)
