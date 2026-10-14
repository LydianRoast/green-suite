#!/usr/bin/env perl
# Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

use strict;
use warnings;
use utf8;
use JSON::PP qw(decode_json);
use Fcntl qw(O_WRONLY O_CREAT O_EXCL);

my @reading_columns = qw(id timestamp group sensor kind value unit source raw);
my @columns = (@reading_columns, qw(points green_suite_csv_version));

# Match Rust model::csv_text. Version 1 imports remove exactly one leading
# apostrophe, preserving both dangerous-looking text and original apostrophes.
sub csv_text {
    my ($text) = @_;
    $text = '' unless defined($text);
    return "'$text" if $text =~ /\A['\t\r\n]/ || $text =~ /\A\p{White_Space}*[=+\-@]/;
    return $text;
}

sub csv_record {
    return join(',', map {
        my $value = defined($_) ? "$_" : '';
        $value =~ s/"/""/g;
        '"' . $value . '"';
    } @_) . "\r\n";
}

sub readings_from_project {
    my ($project) = @_;
    die "Expected a Green Suite project JSON object.\n" unless ref($project) eq 'HASH';
    die "Project has no readings array.\n" unless ref($project->{readings}) eq 'ARRAY';
    my $index = 0;
    for my $reading (@{$project->{readings}}) {
        ++$index;
        die "Reading $index is not an object.\n" unless ref($reading) eq 'HASH';
        for my $column (@reading_columns) {
            die "Reading $index has a structured $column value.\n" if ref($reading->{$column});
        }
        my $value = $reading->{value};
        die "Reading $index has no finite numeric value.\n"
            unless defined($value) && $value =~ /\A[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?\z/;
        my $numeric = 0 + $value;
        die "Reading $index exceeds the finite numeric range.\n"
            if "$numeric" =~ /(?:inf|nan)/i;
    }
    return $project->{readings};
}

sub point_labels {
    my ($project) = @_;
    return {} unless exists($project->{points});
    die "Project points must be an array.\n" unless ref($project->{points}) eq 'ARRAY';
    my %labels;
    for my $point (@{$project->{points}}) {
        die "Mapped point must be an object.\n" unless ref($point) eq 'HASH';
        die "Mapped point label must be text.\n" if ref($point->{label});
        my $ids = $point->{reading_ids} // [];
        die "Mapped point reading_ids must be an array.\n" unless ref($ids) eq 'ARRAY';
        for my $id (@$ids) {
            die "Mapped point reading id must be text.\n" if !defined($id) || ref($id);
            push @{$labels{$id}}, $point->{label} // '';
        }
    }
    return \%labels;
}

sub reading_record {
    my ($reading, $labels) = @_;
    my @values = map {
        # Seventeen significant digits round-trip an IEEE f64 without Perl's
        # default string conversion truncating a stored measurement.
        $_ eq 'value' ? sprintf('%.17g', 0 + $reading->{$_}) : csv_text($reading->{$_});
    } @reading_columns;
    my $points = join('; ', @{$labels->{$reading->{id} // ''} // []});
    return csv_record(@values, csv_text($points), 1);
}

if (@ARGV == 1 && $ARGV[0] eq '--self-test') {
    require Test::More;
    Test::More->import;
    Test::More::is(csv_record('A,B', 'a"b', "two\nlines", '', undef),
        "\"A,B\",\"a\"\"b\",\"two\nlines\",\"\",\"\"\r\n", 'CSV quotes commas, quotes, newlines, and blanks');
    Test::More::is(csv_record('café', 0), "\"café\",\"0\"\r\n", 'Unicode and measured zero survive');
    for my $text ('=1+1', '+1', '-1', '@SUM(A1)', "'literal", "\tfirst", "\rfirst", "\nfirst", '  =HYPERLINK("x")', "\x{a0}=1") {
        Test::More::is(csv_text($text), "'$text", 'formula-like text and original apostrophes use reversible version 1 escaping');
    }
    Test::More::is(csv_text('plain text'), 'plain text', 'ordinary text is unchanged');
    Test::More::is(csv_text(''), '', 'empty text remains empty');
    my $project = {readings => [{id => 'r1', value => -1.25, group => '=1+1', raw => "'raw"}], points => [
        {label => '  @valve', reading_ids => ['r1']}, {label => 'SD-02', reading_ids => ['r1']}]};
    Test::More::is(reading_record($project->{readings}[0], point_labels($project)),
        "\"r1\",\"\",\"'=1+1\",\"\",\"\",\"-1.25\",\"\",\"\",\"''raw\",\"'  \@valve; SD-02\",\"1\"\r\n",
        'version 1 row has numeric value, escaped fields, and associated point labels');
    my $precise = 1.2345678901234567;
    Test::More::like(reading_record({id => 'precision', value => $precise}, {}), qr/1\.2345678901234567/,
        'measurement precision survives export');
    my $readings = readings_from_project({readings => [{value => 0}, {value => -1.25e3}]});
    Test::More::is(scalar(@$readings), 2, 'zero and signed numeric readings are accepted');
    for my $invalid ([], {}, {readings => {}}, {readings => [1]},
        {readings => [{value => 'NaN'}]}, {readings => [{value => '1e999'}]},
        {readings => [{value => 1, sensor => {}}]}) {
        my $accepted = eval { readings_from_project($invalid); 1 };
        Test::More::ok(!$accepted, 'malformed project or reading is rejected');
    }
    Test::More::done_testing();
    exit;
}

my $output_path;
if (@ARGV == 3 && $ARGV[1] eq '--output') {
    $output_path = pop @ARGV;
    pop @ARGV;
}

if (@ARGV != 1 || $ARGV[0] =~ /\A--(?:help|version)\z/) {
    print STDERR "Usage: perl tools/export-readings.pl project.json [--output readings.csv]\n";
    print STDERR "       perl tools/export-readings.pl --self-test\n";
    print STDERR "Reads a saved Green Suite project; emits UTF-8 Green Suite CSV version 1. No device access.\n";
    exit(@ARGV == 1 ? 0 : 2);
}

my $path = $ARGV[0];
open my $input, '<:raw', $path or die "Cannot open '$path': $!\n";
my $size = -s $input;
die "Project exceeds the 128 MiB input limit.\n" if defined($size) && $size > 128 * 1024 * 1024;
my $bytes = do { local $/; <$input> };
close $input or die "Cannot close '$path': $!\n";
my $project = eval { decode_json($bytes) };
die "Invalid project JSON: $@" if $@;
my $readings = readings_from_project($project);
my $labels = point_labels($project);

my $output = \*STDOUT;
if (defined($output_path)) {
    sysopen $output, $output_path, O_WRONLY | O_CREAT | O_EXCL
        or die "Cannot create '$output_path' (existing files are not overwritten): $!\n";
}
binmode $output, ':raw:encoding(UTF-8)' or die "Cannot configure UTF-8 output: $!\n";
print {$output} csv_record(@columns) or die "Cannot write CSV: $!\n";
for my $reading (@$readings) {
    print {$output} reading_record($reading, $labels) or die "Cannot write CSV: $!\n";
}
close $output or die "Cannot finish CSV output: $!\n";
