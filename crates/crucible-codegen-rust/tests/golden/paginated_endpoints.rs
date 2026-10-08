    pub fn read_pages(&self, input: Input) -> ::crucible_client_runtime_rust::Paginator<Read, ReadError> {
        ::crucible_client_runtime_rust::Paginator::<Read, ReadError>::new(
            self.transport.clone(),
            input,
            |output| { let value = output; let value = value.next_token.value()?; Some(value.as_str()) },
            |input| input.next_token.as_deref(),
            |input, token| input.next_token = ::crucible_client_runtime_rust::Field::Value(token),
        ).metadata(&__crucible_metadata_Read)
    }
