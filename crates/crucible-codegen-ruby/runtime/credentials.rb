# frozen_string_literal: true
module CrucibleGenerated
  module Runtime
    # Providers for `ClientConfig#credentials`; each returns nil when it has none.
    module Credentials
      # Reads `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`.
      def self.environment
        access, secret = ENV['AWS_ACCESS_KEY_ID'], ENV['AWS_SECRET_ACCESS_KEY']
        return nil unless access || secret
        raise ::ArgumentError, 'AWS environment credentials are incomplete' unless access && secret
        AwsCredentials.new(access_key_id: access, secret_access_key: secret, session_token: ENV['AWS_SESSION_TOKEN'])
      end
      # Static keys from a shared credentials file; never runs `credential_process`.
      def self.profile(profile: ENV.fetch('AWS_PROFILE', 'default'), path: ENV.fetch('AWS_SHARED_CREDENTIALS_FILE') { ::File.join(::Dir.home, '.aws/credentials') })
        return nil unless ::File.file?(path)
        section, values = nil, {}
        ::File.foreach(path, encoding: 'UTF-8') do |line|
          line = line.strip
          next if line.empty? || line.start_with?('#', ';')
          if (match = /\A\[([^\]]+)\]\z/.match(line))
            section = match[1]
          elsif section == profile && line.include?('=')
            key, value = line.split('=',2); values[key.strip] = value.strip
          end
        end
        access, secret = values['aws_access_key_id'], values['aws_secret_access_key']
        return nil unless access || secret
        raise ::ArgumentError, 'AWS profile credentials are incomplete' unless access && secret
        AwsCredentials.new(access_key_id: access, secret_access_key: secret, session_token: values['aws_session_token'])
      end
      # Tries providers in order and returns the first identity; errors remain errors.
      def self.chain(*providers)
        lambda do
          providers.each { |provider| value = provider.call; return value unless value.nil? }
          nil
        end
      end
    end
  end
end
