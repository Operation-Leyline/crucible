# frozen_string_literal: true
require 'jobs_client'
require 'json'
R = Jobs::Runtime
def check(value, message)
  raise message unless value
end
def rejects(type = ArgumentError)
  yield
rescue type => error
  error
else
  raise "Expected #{type}"
end
class Fake
  attr_reader :requests
  def initialize(&handler)
    @handler, @requests = handler, []
  end
  def send_request(request, read_body:, scope:)
    @requests << request
    @handler.call(@requests.size, request, scope)
  end
end
def json(status, body) = R::HttpResponse.new(status, body: JSON.generate(body))
def jobs(fake) = Jobs::JobsClient.new(R::ClientConfig.new(endpoint: 'https://example.invalid', transport: fake, max_attempts: 1))
input = Jobs::Input.new(names: ['a', 'b'])
fast = {min_delay: 0, max_delay: 0}

fake = Fake.new { |count| count == 1 ? json(404, '__type' => 'Missing') : json(200, 'wire-status' => count >= 3 ? 'READY' : 'PENDING') }
output = jobs(fake).wait_until_ready(input, timeout: 2, **fast)
check(output.status == 'READY' && fake.requests.size == 3, 'Retry acceptor, pending output, then success')

fake = Fake.new { |count| json(200, 'wire-status' => count == 1 ? 'PENDING' : 'READY') }
output = jobs(fake).wait_until_ready(input, timeout: 1.5, min_delay: 1, max_delay: 1)
check(output.status == 'READY' && fake.requests.size == 2, 'Nonzero delays reserve budget for one final transport poll')

fake = Fake.new { json(404, '__type' => 'Missing') }
check(jobs(fake).wait_until_deleted(input, timeout: 1).nil? && fake.requests.size == 1, 'Error type success acceptor')

payload = {'items' => [{'wire-status' => 'READY'}, {'wire-status' => 'READY'}], 'updatedAt' => '2026-01-01T00:00:00Z'}
fake = Fake.new { json(200, payload) }
client = jobs(fake)
%i[wait_until_all_ready wait_until_any_ready wait_until_filtered_ready wait_until_counts_match wait_until_updated wait_until_functions].each do |name|
  client.public_send(name, input, timeout: 1)
end
check(fake.requests.size == 6 && input.names == ['a', 'b'], 'Projection, filter, input/output, timestamp, and function acceptors')
fake = Fake.new { json(200, 'items' => [{'wire-status' => 'READY'}], 'updatedAt' => '1970-01-01T00:00:00Z') }
check(rejects(R::WaiterTimeoutError) { jobs(fake).wait_until_counts_match(input, timeout: 0.05) }.attempts == 1, 'Input/output length mismatch keeps polling')

fake = Fake.new { json(200, 'wire-status' => 'FAILED', 'reasons' => ['job rejected']) }
error = rejects(R::WaiterError) { jobs(fake).wait_until_ready(input, timeout: 1) }
check(!error.is_a?(R::WaiterTimeoutError) && error.attempts == 1 && error.reason == ['job rejected'] && error.message.include?('job rejected'), 'Failure acceptor message')

fake = Fake.new { json(403, '__type' => 'Denied') }
error = rejects(R::WaiterError) { jobs(fake).wait_until_ready(input, timeout: 1) }
check(error.error.metadata.name == 'Denied' && error.attempts == 1, 'Unmatched error fails the waiter')

fake = Fake.new { json(200, 'wire-status' => 'PENDING') }
start = R::CallScope.now
error = rejects(R::WaiterTimeoutError) { jobs(fake).wait_until_ready(input, timeout: 0.05) }
check(error.attempts == 1 && fake.requests.size == 1 && R::CallScope.now - start < 0.5, 'Last delay is clipped to the deadline')

fake = Fake.new do |_count, _request, scope|
  loop do
    scope.check
    sleep 0.005
  end
end
start = R::CallScope.now
rejects(R::WaiterTimeoutError) { jobs(fake).wait_until_ready(input, timeout: 0.05) }
check(R::CallScope.now - start < 0.5, 'Waiter deadline bounds an in-flight attempt')

cancelled = R::CancellationToken.new
cancelled.cancel
fake = Fake.new { json(200, 'wire-status' => 'READY') }
rejects(R::CancelledError) { jobs(fake).wait_until_ready(input, timeout: 1, cancellation: cancelled) }
check(fake.requests.empty?, 'Precancelled waiter sends nothing')
fake = Fake.new { json(200, 'wire-status' => 'PENDING') }
start = R::CallScope.now
token = R::CancellationToken.new { R::CallScope.now - start > 0.05 }
rejects(R::CancelledError) { jobs(fake).wait_until_ready(input, timeout: 10, min_delay: 5, max_delay: 5, cancellation: token) }
check(R::CallScope.now - start < 1, 'Cancellation interrupts the delay')
rejects { jobs(fake).wait_until_ready(input, timeout: 0) }
rejects { jobs(fake).wait_until_ready(input, timeout: 1, min_delay: 3, max_delay: 2) }

class Ceiling
  def self.rand = 1.0
end
waiter = R::Waiter.new('Test', {'minDelay' => 2, 'maxDelay' => 8, 'acceptors' => []})
check((1..5).map { |attempt| waiter.delay(attempt, 100, random: Ceiling) } == [2, 4, 8, 8, 8], 'Exponential ceiling')
check(waiter.delay(2, 5, random: Ceiling) == 3, 'Delay near deadline reserves one minimum delay for a final call')
check(waiter.delay(2, 2, random: Ceiling) == 2 && waiter.delay(2, 1, random: Ceiling) == 1, 'Short remaining budgets wait out the remainder')
check((1..50).all? { waiter.delay(3, 100).between?(2, 8) }, 'Jitter stays between bounds')

literal = ->(value) { {'kind' => 'literal', 'value' => value} }
call = ->(name, *args) { R::Query.search({'kind' => 'function', 'function' => name, 'arguments' => args}, nil) }
check(call.('to_number', literal.('12345678901234567890.0000000001')) == Rational('12345678901234567890.0000000001'), 'Exact to_number')
check(call.('contains', literal.([true]), literal.(1)) == false, 'Booleans are not numbers')
check(call.('sort', literal.([R::JsonNumber.new('2.2'), R::JsonNumber.new('1.1')])) == [Rational('1.1'), Rational('2.2')], 'Exact numeric sort')
check(call.('type', literal.(R::JsonNumber.new('1.1'))) == 'number', 'Decimal type')
check(call.('to_string', literal.({'number' => R::JsonNumber.new('1.000000000000000000001')})) == '{"number":1.000000000000000000001}', 'Exact to_string')
rejects(R::QueryError) { call.('abs', literal.('x')) }
values = [1, 2, 3, 4, 5]
[
  [{'start' => 1, 'stop' => 4, 'step' => nil}, [2, 3, 4]],
  [{'start' => 1, 'stop' => 4}, [2, 3, 4]],
  [{'start' => nil, 'stop' => nil, 'step' => nil}, values],
  [{'start' => -99, 'stop' => 99, 'step' => 2}, [1, 3, 5]],
  [{'start' => 99, 'stop' => -99, 'step' => -2}, [5, 3, 1]],
  [{'start' => nil, 'stop' => -1, 'step' => -1}, []]
].each do |bounds, expected|
  check(R::Query.search({'kind' => 'slice'}.merge(bounds), values) == expected.map { |n| Rational(n) }, "Slice bounds #{bounds}")
end
check(R::Query.search({'kind' => 'slice', 'step' => nil}, []) == [], 'Empty slice with default step')
[0, Rational(3, 2), false, '2'].each do |step|
  rejects(R::QueryError) { R::Query.search({'kind' => 'slice', 'step' => step}, values) }
end
check(R::Query.search({'kind' => 'slice', 'start' => nil, 'stop' => nil, 'step' => -2}, [1, 2, 3, 4, 5]) == [5, 3, 1].map { |n| Rational(n) }, 'Negative slice')
puts 'Ruby waiters, compiled JMESPath, deadlines, jitter, and cancellation passed'
