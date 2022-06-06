use std::fs::File;
use std::io;
use std::io::{BufReader, BufWriter, stdout, Stdout, Write};
use std::path::Path;
use csv::{DeserializeRecordsIter, Reader, Trim};
use crate::domain::{Account, TransactionRequest};

pub struct ReportProducer {
   writer: BufWriter<Stdout>
}

impl ReportProducer {
    pub fn new() -> Self {
        let mut writer = BufWriter::new(stdout());
        match writeln!(writer, "client, available, held, total, locked") {
            Ok(_) => (),
            Err(_) => {
                eprintln!("Error writing report to sdout!, will continue to work regardless.");
            }
        }
        ReportProducer {
            writer
        }
    }

    pub fn add(&mut self, account: &Account) {
        match writeln!(self.writer, "{},{},{},{},{}",
                        account.client_id(),
                        account.available(),
                        account.held(),
                        account.total(),
                        account.is_locked(),
        ) {
            Ok(_) => (),
            Err(_) => {
                eprintln!("Error writing entry to sdout!, will continue to work regardless.");
            }
        }
    }

}

impl Drop for ReportProducer {
    fn drop(&mut self) {
        match self.writer.flush() {
            Ok(_) => (),
            Err(_) => {
                eprintln!("Error flushing report to sdout!, will continue to work regardless.");
            }
        }
    }
}

pub struct TransactionFileReader {
    reader: Reader<BufReader<File>>,
}

impl TransactionFileReader {

    pub fn from<F>(filename: F) -> Result<Self, io::Error> where F: AsRef<Path> {

        let file = File::open(filename)?;
        let buf_reader = BufReader::new(file);
        let csv_reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .double_quote(false)
            .quoting(false)
            .flexible(true)
            .trim(Trim::All)
            .delimiter(b',')
            .from_reader(buf_reader);

        Ok(TransactionFileReader {
            reader: csv_reader
        })
    }

    pub fn values(&mut self) -> Visitor {
        Visitor {
            iter: self.reader.deserialize::<TransactionRequest>()
        }
    }
}

pub struct Visitor<'a> {
    iter: DeserializeRecordsIter<'a, BufReader<File>, TransactionRequest>
}

impl <'a> Iterator for Visitor<'a> {
    type Item = TransactionRequest;

    fn next(&mut self) -> Option<Self::Item> {
        let opt = self.iter.next();
        match opt {
            None => None,
            Some(result) => match result {
                Ok(tx) => Some(tx),
                Err(err) => {
                    eprintln!("Error while reading transaction request. Will skip the record and continue. {}", err);
                    None
                }
            }
        }
    }
}

