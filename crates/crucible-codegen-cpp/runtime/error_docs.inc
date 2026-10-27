            modeled = apply_headers(matches[0], *traits, response, modeled);
            if (documentation) {
                const auto* member = documentation->find("reasonMember");
                if (member && string(*member)) {
                    const auto* reason = modeled.find(member->string());
                    if (!reason) reason = documentation->find("reasonDefault");
                    if (reason && string(*reason)) {
                        const auto* url = documentation->at("reasonUrls").find(reason->string());
                        if (url) metadata.doc_url = url->string();
                    }
                }
            }
            const auto error = errors_(matches[0], modeled, metadata); if (error) return {metadata, error};
