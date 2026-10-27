        if (!code) {
            result->error_shape = descriptor;
            const yyjson_val *member = get(documentation, "reasonMember");
            if (yyjson_is_str(member)) {
                string key = text(member);
                const yyjson_val *reason = yyjson_obj_getn((yyjson_val *)value, key.data, key.length);
                if (!reason) reason = get(documentation, "reasonDefault");
                if (yyjson_is_str(reason)) {
                    string literal = text(reason);
                    const yyjson_val *url = yyjson_obj_getn((yyjson_val *)get(documentation, "reasonUrls"), literal.data, literal.length);
                    /* Schema strings belong to the client, while results outlive it. */
                    if (yyjson_is_str(url)) { TRY(crucible_generated_arena_copy(storage, text(url), &result->doc_url, issue)); }
                }
            }
        }
