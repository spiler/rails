#!/bin/bash
# Executing in docker is a little different because it will mount an empty file if the argument is a file that does not
# exist.
# The program verifies that the argument is a file and it is readable, but for convenience added this bash trickery.
if [ -z "$1" ]
  then
    echo "No file supplied. Expected:"
    echo "./image-run.sh <filename>"
  else
    if [ -s diff.txt ]
      then
        docker run --volume `pwd`/$1:/etc/rails/input.csv -ti challenge/rails:1.0
      else
        echo "File $1 does not exist. Expected:"
        echo "./image-run.sh <filename>"
    fi
fi
