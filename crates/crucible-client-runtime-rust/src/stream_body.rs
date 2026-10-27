//! Pulls one requested item and validates terminal EOF before completion.
use crate::stream_event::StreamEvent;
use crate::stream_parser::StreamParser;
use crate::{Error, OperationErrors, ResponseMetadata, ServiceError, UnknownError};
use bytes::Bytes;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(crate) struct StreamBody {
    response: reqwest::Response,
    parser: StreamParser,
    chunk: Bytes,
    eof: bool,
}
impl StreamBody {
    pub(crate) fn new(response: reqwest::Response, limit: usize) -> Self {
        Self {
            response,
            parser: StreamParser::new(limit),
            chunk: Bytes::new(),
            eof: false,
        }
    }
    async fn event<E>(&mut self) -> Result<Option<StreamEvent>, Error<E>> {
        loop {
            if let Some(event) = self.parser.next(&mut self.chunk).map_err(Error::Decode)? {
                return Ok(Some(event));
            }
            if self.eof {
                return self.parser.finish().map_err(Error::Decode);
            }
            match self.response.chunk().await.map_err(Error::Transport)? {
                Some(chunk) => self.chunk = chunk,
                None => self.eof = true,
            }
        }
    }
    async fn terminal_eof<E>(&mut self) -> Result<(), Error<E>> {
        self.parser
            .skip_lf(&mut self.chunk)
            .map_err(Error::Decode)?;
        if !self.chunk.is_empty() {
            return Err(Error::Decode("data after terminal stream outcome".into()));
        }
        if !self.eof {
            while let Some(mut chunk) = self.response.chunk().await.map_err(Error::Transport)? {
                self.parser.skip_lf(&mut chunk).map_err(Error::Decode)?;
                if !chunk.is_empty() {
                    return Err(Error::Decode("data after terminal stream outcome".into()));
                }
            }
            self.eof = true;
        }
        Ok(())
    }
    pub(crate) async fn next<T: DeserializeOwned, E: OperationErrors>(
        &mut self,
        metadata: &ResponseMetadata,
    ) -> Result<Option<T>, Error<E>> {
        let Some(event) = self.event().await? else {
            return Err(Error::Decode(
                "response stream ended without a terminal outcome".into(),
            ));
        };
        if event.kind.is_empty() || event.kind == "message" {
            if event.data.trim_ascii() == b"null" {
                return Err(Error::Decode("invalid modeled stream item".into()));
            }
            return serde_json::from_slice(&event.data)
                .map(Some)
                .map_err(|_| Error::Decode("invalid modeled stream item".into()));
        }
        let value: Value = serde_json::from_slice(&event.data)
            .map_err(|_| Error::Decode("invalid JSON in response stream".into()))?;
        match event.kind.as_str() {
            "crucible.complete" => {
                if !value.as_object().is_some_and(|v| v.is_empty()) {
                    return Err(Error::Decode("invalid stream completion".into()));
                }
                self.terminal_eof().await?;
                Ok(None)
            }
            "crucible.error" => {
                let object = value
                    .as_object()
                    .ok_or_else(|| Error::Decode("invalid terminal stream error".into()))?;
                let name = object
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|n| !n.is_empty())
                    .ok_or_else(|| Error::Decode("invalid terminal stream error".into()))?;
                let status = object
                    .get("status")
                    .and_then(Value::as_u64)
                    .filter(|s| (400..600).contains(s))
                    .ok_or_else(|| Error::Decode("invalid terminal stream error".into()))?
                    as u16;
                let fields = object
                    .get("error")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| Error::Decode("invalid terminal stream error".into()))?;
                let bytes = serde_json::to_vec(fields).expect("parsed JSON serializes");
                let metadata = Box::new(ResponseMetadata {
                    status,
                    ..metadata.clone()
                });
                let error = match E::decode_document(status, name, &bytes) {
                    Some(error) => Error::Service(ServiceError { error, metadata }),
                    None => {
                        let info = E::classify(status, Some(name));
                        Error::Unknown(UnknownError {
                            metadata,
                            type_name: Some(name.to_owned()),
                            message: fields
                                .get("message")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            retryable: info.is_some_and(|i| i.retryable),
                            throttling: info.is_some_and(|i| i.throttling),
                        })
                    }
                };
                self.terminal_eof().await?;
                Err(error)
            }
            _ => Err(Error::Decode("unsupported stream event".into())),
        }
    }
}
