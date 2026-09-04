# Flattens NOAA HURDAT2 best-track data into a single headed CSV.
#
# HURDAT2 interleaves a per-storm header line (basin id, name, track length) before that storm's
# track rows, so no single header describes the file. This script carries each storm's id and name
# down onto its rows, and reformats the packed YYYYMMDD date and HHMM time into separator-bearing
# strings ("2005-08-25", "12:00"). The separators matter: they keep CSV type inference reading the
# two columns as text, so the mapping's datetime template can compose them into an RFC 3339 instant
# rather than the columns being coerced to bare numbers.
#
# Usage: awk -f flatten.awk hurdat2-atlantic.txt > cyclones.csv

BEGIN {
    FS = ","
    OFS = ","
    print "storm_id,name,date,time,record_id,status,lat,lon,max_wind,min_pressure"
}

# A header line names the next storm: its first field is the alphabetic basin id (AL, EP, CP).
$1 ~ /^[[:space:]]*[A-Z][A-Z]/ {
    storm_id = trim($1)
    name = trim($2)
    next
}

# Every other line is one track observation of the current storm.
{
    date = trim($1)
    time = trim($2)
    iso_date = substr(date, 1, 4) "-" substr(date, 5, 2) "-" substr(date, 7, 2)
    iso_time = substr(time, 1, 2) ":" substr(time, 3, 2)
    print storm_id, name, iso_date, iso_time, trim($3), trim($4), trim($5), trim($6), trim($7), trim($8)
}

function trim(value) {
    gsub(/^[ \t]+|[ \t]+$/, "", value)
    return value
}
